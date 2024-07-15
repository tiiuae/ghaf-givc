// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package grpc

import (
	"context"
	"time"

	"givc/internal/pkgs/types"
	givc_util "givc/internal/pkgs/utility"

	"github.com/grpc-ecosystem/go-grpc-middleware/v2/interceptors/retry"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
	grpc_codes "google.golang.org/grpc/codes"
	grpc_creds "google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	grpc_metadata "google.golang.org/grpc/metadata"
)

var (
	MAX_RETRY          = uint(7)
	TIMEOUT_LONG       = 10 * time.Second
	TIMEOUT_SHORT      = 3 * time.Second
	BACKOFF_TIME_LONG  = 200 * time.Millisecond
	BACKOFF_TIME_SHORT = 50 * time.Millisecond
)

func NewClient(cfg *types.EndpointConfig, allowLongWaits bool) (*grpc.ClientConn, error) {

	// @TODO Input validation

	options := []grpc.DialOption{}

	// Create client tls config
	tlsConfig := givc_util.TlsClientConfigFromTlsConfig(cfg.TlsConfig, cfg.Transport.Name)

	// Setup TLS credentials
	var tlsCredentials grpc.DialOption
	if tlsConfig != nil {
		tlsCredentials = grpc.WithTransportCredentials(grpc_creds.NewTLS(tlsConfig))
	} else {
		tlsCredentials = grpc.WithTransportCredentials(insecure.NewCredentials())
		log.Warning("TLS configuration not provided, using insecure connection")
	}
	options = append(options, tlsCredentials)

	// Retry options
	retryOpts := []retry.CallOption{
		retry.WithCodes(grpc_codes.NotFound, grpc_codes.Unavailable, grpc_codes.Aborted),
		retry.WithMax(MAX_RETRY),
	}
	if allowLongWaits {
		retryOpts = append(retryOpts, retry.WithPerRetryTimeout(TIMEOUT_LONG), retry.WithBackoff(retry.BackoffExponential(BACKOFF_TIME_LONG)))
	} else {
		retryOpts = append(retryOpts, retry.WithPerRetryTimeout(TIMEOUT_SHORT), retry.WithBackoff(retry.BackoffExponential(BACKOFF_TIME_SHORT)))
	}

	// Setup GRPC config
	interceptors := []grpc.UnaryClientInterceptor{
		withOutgoingContext,
		retry.UnaryClientInterceptor(retryOpts...),
	}
	options = append(options, grpc.WithChainUnaryInterceptor(interceptors...))

	return grpc.NewClient(cfg.Transport.Address+":"+cfg.Transport.Port, options...)
}

func withOutgoingContext(ctx context.Context, method string, req, resp interface{}, cc *grpc.ClientConn, invoker grpc.UnaryInvoker, opts ...grpc.CallOption) error {

	var outmd grpc_metadata.MD
	if md, ok := grpc_metadata.FromOutgoingContext(ctx); ok {
		outmd = md.Copy()
	}

	ctx = grpc_metadata.NewOutgoingContext(context.Background(), outmd)

	return invoker(ctx, method, req, resp, cc, opts...)
}
