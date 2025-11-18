// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// The grpc package provides functionality to create and manage gRPC server and client connections.
package grpc

import (
	"context"
	"fmt"
	"net"
	"strings"
	"time"

	"givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"

	"github.com/grpc-ecosystem/go-grpc-middleware/v2/interceptors/retry"
	vsock "github.com/linuxkit/virtsock/pkg/vsock"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"

	grpc_codes "google.golang.org/grpc/codes"
	grpc_creds "google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	grpc_metadata "google.golang.org/grpc/metadata"
)

// Constants for gRPC client configuration
const (
	MAX_RETRY = uint(3)
	TIMEOUT   = 150 * time.Millisecond
)

// NewClient creates a new gRPC client connection based on the provided endpoint configuration.
func NewClient(cfg *types.EndpointConfig) (*grpc.ClientConn, error) {

	// @TODO Input validation

	options := []grpc.DialOption{}

	// Create client tls config
	tlsConfig, err := givc_util.TlsClientConfigFromTlsConfig(cfg.TlsConfig, cfg.Transport.Name)
	if err != nil {
		return nil, fmt.Errorf("unable to create TLS client config: %v", err)
	}

	// Setup TLS credentials
	var tlsCredentials grpc.DialOption
	if tlsConfig != nil {
		tlsCredentials = grpc.WithTransportCredentials(grpc_creds.NewTLS(tlsConfig))
	} else {
		tlsCredentials = grpc.WithTransportCredentials(insecure.NewCredentials())
		log.Warning("TLS configuration not provided, using insecure connection")
	}
	options = append(options, tlsCredentials)

	// Setup GRPC config
	interceptors := []grpc.UnaryClientInterceptor{
		withOutgoingContext,
		withRetryOpts(),
	}
	options = append(options, grpc.WithChainUnaryInterceptor(interceptors...))

	// Set address
	var addr string
	switch cfg.Transport.Protocol {
	case "tcp":
		addr = cfg.Transport.Address + ":" + cfg.Transport.Port
	case "vsock":
		addr = "passthrough:vsock:" + cfg.Transport.Address + ":" + cfg.Transport.Port
		options = append(options, grpc.WithContextDialer(vsockDialer))
	case "unix":
		addr = cfg.Transport.Address
	default:
		return nil, fmt.Errorf("unsupported protocol: %s", cfg.Transport.Protocol)
	}

	return grpc.NewClient(addr, options...)
}

// withOutgoingContext is a gRPC client interceptor that adds outgoing metadata to the context.
func withOutgoingContext(ctx context.Context, method string, req, resp interface{}, cc *grpc.ClientConn, invoker grpc.UnaryInvoker, opts ...grpc.CallOption) error {

	var outmd grpc_metadata.MD
	if md, ok := grpc_metadata.FromOutgoingContext(ctx); ok {
		outmd = md.Copy()
	}
	ctx = grpc_metadata.NewOutgoingContext(context.Background(), outmd)

	return invoker(ctx, method, req, resp, cc, opts...)
}

// withRetryOpts is a gRPC client interceptor that adds retry options to the gRPC call.
func withRetryOpts() grpc.UnaryClientInterceptor {
	retryOpts := []retry.CallOption{
		retry.WithCodes(grpc_codes.NotFound, grpc_codes.Unavailable, grpc_codes.Aborted),
		retry.WithMax(MAX_RETRY),
		retry.WithPerRetryTimeout(TIMEOUT),
	}
	return retry.UnaryClientInterceptor(retryOpts...)
}

// vsockDialer is a custom dialer for vsock connections.
func vsockDialer(ctx context.Context, addr string) (net.Conn, error) {
	log.Infof("Dialing vsock: %s", addr)

	cid, port, err := givc_util.ParseVsockAddress(strings.TrimPrefix(addr, "vsock:"))
	if err != nil {
		return nil, fmt.Errorf("unable to parse vsock address: %v", err)
	}
	dialConn, err := vsock.Dial(cid, port)
	if err != nil {
		return nil, fmt.Errorf("unable to connect to vsock: %s", addr)
	}
	conn, ok := dialConn.(net.Conn)
	if !ok {
		return nil, fmt.Errorf("unable to convert vsock connection to net.Conn")
	}
	return conn, nil
}
