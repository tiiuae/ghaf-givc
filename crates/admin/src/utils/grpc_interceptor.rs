// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::Context as _;
use bytes::{Buf, Bytes, BytesMut};
use http_body::Body as _;
use http_body::Frame;
use http_body_util::StreamBody;
use prost_reflect::{DescriptorPool, DynamicMessage};
use tokio_stream::Stream;
use tonic::Status;
use tower::{Layer, Service};

pub trait GrpcInterceptor: Clone + Send + 'static {
    /// Intercept and Grpc message or stream (first message)
    ///
    /// # Errors
    /// Should return error if request should be rejeceted
    fn intercept(
        &mut self,
        parts: &http::request::Parts,
        service: &str,
        method: &str,
        message: &DynamicMessage,
    ) -> Result<(), Status>;
}

#[derive(Clone)]
pub struct GrpcInterceptorLayer<I> {
    pool: Arc<DescriptorPool>,
    interceptor: I,
    max_request_size: usize,
}

impl<I> GrpcInterceptorLayer<I> {
    /// Creates new `GrpcInterceptorLayer`
    ///
    /// # Errors
    /// Fails if protocol descriptors can not be decoded
    pub fn new<'a>(
        file_descriptor_set: impl IntoIterator<Item = &'a [u8]>,
        interceptor: I,
    ) -> anyhow::Result<Self> {
        let mut pool = DescriptorPool::new();
        for fds in file_descriptor_set {
            pool.decode_file_descriptor_set(fds)
                .context("failed to decode file descriptor set")?;
        }
        Ok(Self {
            pool: Arc::new(pool),
            interceptor,
            // Default tonic/prost limit
            max_request_size: 4 * 1024 * 1024,
        })
    }

    #[must_use]
    pub fn set_max_request_size(self, max_request_size: usize) -> Self {
        Self {
            max_request_size,
            ..self
        }
    }
}

impl<S, I> Layer<S> for GrpcInterceptorLayer<I>
where
    I: Clone,
{
    type Service = GrpcInterceptorService<S, I>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcInterceptorService {
            inner,
            pool: self.pool.clone(),
            interceptor: self.interceptor.clone(),
            max_request_size: self.max_request_size,
        }
    }
}

#[derive(Clone)]
pub struct GrpcInterceptorService<S, I> {
    inner: S,
    pool: Arc<DescriptorPool>,
    interceptor: I,
    max_request_size: usize,
}

impl<S, I> Service<http::Request<tonic::body::Body>> for GrpcInterceptorService<S, I>
where
    S: Service<http::Request<tonic::body::Body>, Response = http::Response<tonic::body::Body>>
        + Send
        + Clone
        + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
    S::Future: Send + 'static,
    I: GrpcInterceptor,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<tonic::body::Body>) -> Self::Future {
        let pool = self.pool.clone();
        let mut svc = self.inner.clone();
        let mut interceptor = self.interceptor.clone();
        let max_request_size = self.max_request_size;

        Box::pin(async move {
            let (parts, mut body) = req.into_parts();
            let path = parts.uri.path();
            let Some((input_type, service, method)) =
                parse_grpc_path(path).and_then(|(service, method)| {
                    pool.get_service_by_name(service)
                        .and_then(|s| s.methods().find(|m| m.name() == method))
                        .map(|m| (m.input(), service, method))
                })
            else {
                return Ok(Status::invalid_argument("unknown method").into_http());
            };

            let mut accumulated = BytesMut::new();
            let (accumulated_bytes, payload) = match loop {
                let frame = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;

                match frame.map(|f| f.map(Frame::into_data)) {
                    Some(Ok(Ok(data))) => {
                        accumulated.extend_from_slice(&data);

                        let bytes = accumulated.freeze();
                        {
                            let mut buf = bytes.clone();
                            if let Ok(_compressed) = buf.try_get_u8()
                                && let Ok(len) = buf.try_get_u32()
                            {
                                if len as usize > max_request_size {
                                    break Err(Status::out_of_range("Payload too large"));
                                }

                                if let Some(data) = buf.chunk().get(0..len as usize) {
                                    break Ok((bytes, Some(buf.slice_ref(data))));
                                }
                            }
                        }

                        accumulated = if let Ok(bytes_mut) = bytes.try_into_mut() {
                            bytes_mut
                        } else {
                            unreachable!("bytes is the only reference");
                        };
                    }
                    Some(Ok(Err(_)) | Err(_)) => {
                        break Err(Status::internal("Error receiving data"));
                    }
                    None => break Err(Status::invalid_argument("Incomplete frame data")),
                }
            } {
                Ok(data) => data,
                Err(e) => return Ok(e.into_http()),
            };

            let msg = payload
                .and_then(|payload| DynamicMessage::decode(input_type.clone(), payload).ok())
                .unwrap_or(DynamicMessage::new(input_type));

            if let Err(status) = interceptor.intercept(&parts, service, method, &msg) {
                return Ok(status.into_http());
            }

            let first = Some(Ok(accumulated_bytes));
            let new_body =
                tonic::body::Body::new(StreamBody::new(ChainStream { first, inner: body }));
            let req = http::Request::from_parts(parts, new_body);
            svc.call(req).await
        })
    }
}

struct ChainStream {
    first: Option<Result<Bytes, Status>>,
    inner: tonic::body::Body,
}

impl Stream for ChainStream {
    type Item = Result<Frame<Bytes>, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(result) = self.first.take() {
            return Poll::Ready(Some(result.map(Frame::data)));
        }
        Pin::new(&mut self.inner).poll_frame(cx)
    }
}

fn parse_grpc_path(path: &str) -> Option<(&str, &str)> {
    let mut parts = path.trim_matches('/').split('/');
    parts.next().zip(parts.next())
}
