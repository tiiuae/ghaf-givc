// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package grpc

import (
	"context"
	"crypto/tls"
	"fmt"
	"givc/internal/pkgs/types"
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

	log "github.com/sirupsen/logrus"
)

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

	// GRPC Server
	srv.grpcServer = grpc.NewServer(
		grpc.UnaryInterceptor(
			grpc_middleware.ChainUnaryServer(
				grpc_ctxtags.UnaryServerInterceptor(grpc_ctxtags.WithFieldExtractor(grpc_ctxtags.TagBasedRequestFieldExtractor("log"))),
				grpc.UnaryServerInterceptor(unaryLogRequestInterceptor),
				grpc_logrus.UnaryServerInterceptor(log.NewEntry(log.StandardLogger())),
			),
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

func (s *GrpcServer) ListenAndServe(ctx context.Context, started chan struct{}) error {

	var err error
	var listener net.Listener
	addr := s.config.Transport.Address + ":" + s.config.Transport.Port
	for i := 0; i < LISTENER_RETRIES; i++ {
		listener, err = net.Listen(s.config.Transport.Protocol, addr)
		if err != nil {
			time.Sleep(LISTENER_WAIT_TIME)
			log.WithFields(log.Fields{"addr": addr}).Info("Error binding address for GRPC server, retrying...")
			continue
		}
		break
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
