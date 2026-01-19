// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package grpc

import (
	"context"
	"crypto/tls"
	"fmt"
	"givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"
	"net"
	"time"

	grpc_middleware "github.com/grpc-ecosystem/go-grpc-middleware"
	grpc_logrus "github.com/grpc-ecosystem/go-grpc-middleware/logging/logrus"
	grpc_ctxtags "github.com/grpc-ecosystem/go-grpc-middleware/tags"

	"golang.org/x/sync/errgroup"
	grpc "google.golang.org/grpc"
	grpc_creds "google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/reflection"

	vsock "github.com/linuxkit/virtsock/pkg/vsock"
	log "github.com/sirupsen/logrus"
)

// Constants for gRPC server configuration
const (
	LISTENER_WAIT_TIME = 1 * time.Second
	LISTENER_RETRIES   = 20
)

type GrpcServerConfig struct {
	Transport *types.TransportConfig
	TlsConfig *tls.Config
	Services  []types.GrpcServiceRegistration
}

type GrpcServer struct {
	config     *GrpcServerConfig
	grpcServer *grpc.Server
}

// NewServer creates a new gRPC server based on the provided endpoint configuration and service registrations.
func NewServer(cfg *types.EndpointConfig, services []types.GrpcServiceRegistration) (*GrpcServer, error) {

	// GRPC Server
	srv := GrpcServer{
		config: &GrpcServerConfig{
			Transport: &cfg.Transport,
			TlsConfig: cfg.TlsConfig,
			Services:  services,
		},
	}

	// TLS gRPC creds option
	var grpcTlsConfig grpc.ServerOption
	if srv.config.TlsConfig != nil {
		grpcTlsConfig = grpc.Creds(grpc_creds.NewTLS(srv.config.TlsConfig))
	} else {
		grpcTlsConfig = grpc.Creds(insecure.NewCredentials())
		// return nil, grpc_status.Error(grpc_codes.Unavailable, "TLS configuration not provided")
	}

	// Interceptor chain
	interceptors := []grpc.UnaryServerInterceptor{
		grpc_ctxtags.UnaryServerInterceptor(grpc_ctxtags.WithFieldExtractor(grpc_ctxtags.TagBasedRequestFieldExtractor("log"))),
		unaryLogRequestInterceptor,
		grpc_logrus.UnaryServerInterceptor(log.NewEntry(log.StandardLogger())),
	}
	if srv.config.TlsConfig != nil {
		interceptors = append(interceptors, givc_util.CertIPVerifyInterceptor)
	}

	// GRPC Server
	srv.grpcServer = grpc.NewServer(
		grpc.UnaryInterceptor(
			grpc_middleware.ChainUnaryServer(interceptors...),
		),
		grpcTlsConfig,
	)

	// Register gRPC services
	for _, s := range srv.config.Services {
		log.Info("Registering service: ", s.Name())
		s.RegisterGrpcService(srv.grpcServer)
	}
	reflection.Register(srv.grpcServer)

	return &srv, nil
}

// ListenAndServe starts the gRPC server and listens for incoming connections.
func (s *GrpcServer) ListenAndServe(ctx context.Context, started chan struct{}) error {

	var err error
	var listener net.Listener
	var addr string
	var cid uint32
	var port uint32

	// Set address
	switch s.config.Transport.Protocol {
	case "tcp":
		addr = s.config.Transport.Address + ":" + s.config.Transport.Port
	case "vsock":
		addr = s.config.Transport.Address + ":" + s.config.Transport.Port
		cid, port, err = givc_util.ParseVsockAddress(addr)
		if err != nil {
			return fmt.Errorf("unable to parse vsock address: %v", err)
		}
	case "unix":
		addr = s.config.Transport.Address
	default:
		return fmt.Errorf("unsupported protocol: %s", s.config.Transport.Protocol)
	}

	for i := 0; i < LISTENER_RETRIES; i++ {
		if s.config.Transport.Protocol == "vsock" {
			listener, err = vsock.Listen(cid, port)
		} else {
			listener, err = net.Listen(s.config.Transport.Protocol, addr)
		}
		if err != nil {
			time.Sleep(LISTENER_WAIT_TIME)
			log.WithFields(log.Fields{"addr": addr, "err": err}).Info("Error starting listener for GRPC server, retrying...")
			continue
		}
		break
	}
	if listener == nil {
		return fmt.Errorf("unable to bind address for GRPC server: %s", addr)
	}
	defer listener.Close()

	group, ctx := errgroup.WithContext(ctx)
	idleConnsClosed := make(chan struct{})
	go func() {
		<-ctx.Done()
		s.grpcServer.GracefulStop()
		close(idleConnsClosed)
	}()

	group.Go(func() error {
		log.WithFields(log.Fields{"addr": listener.Addr().String()}).Info("Starting GRPC server")
		close(started)
		err := s.grpcServer.Serve(listener)
		if err != nil {
			return err
		}
		log.WithFields(log.Fields{"addr": listener.Addr().String()}).Info("GRPC server stopped")
		return nil
	})

	<-idleConnsClosed

	if err := group.Wait(); err != nil {
		return fmt.Errorf("GRPC Server error: %s", err)
	}

	return nil
}

func unaryLogRequestInterceptor(ctx context.Context, req interface{}, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (interface{}, error) {
	log.WithFields(grpc_ctxtags.Extract(ctx).Values()).Info("GRPC Request: ", info.FullMethod)
	return handler(ctx, req)
}
