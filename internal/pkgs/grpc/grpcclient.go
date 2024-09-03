// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package grpc

import (
	"context"
	"fmt"
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
	MAX_RETRY_SHORT = uint(10)
	MAX_RETRY_LONG  = uint(60)
	TIMEOUT_SHORT   = 50 * time.Millisecond
	TIMEOUT_LONG    = 1 * time.Second
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
	retries := MAX_RETRY_SHORT
	timeout := TIMEOUT_SHORT
	if allowLongWaits {
		retries = MAX_RETRY_LONG
		timeout = TIMEOUT_LONG
	}
	retryOpts := []retry.CallOption{
		retry.WithCodes(grpc_codes.NotFound, grpc_codes.Unavailable, grpc_codes.Aborted),
		retry.WithMax(retries),
		retry.WithPerRetryTimeout(timeout),
	}

	// Setup GRPC config
	interceptors := []grpc.UnaryClientInterceptor{
		withOutgoingContext,
		retry.UnaryClientInterceptor(retryOpts...),
	}
	options = append(options, grpc.WithChainUnaryInterceptor(interceptors...))

	// Set address
	var addr string
	switch cfg.Transport.Protocol {
	case "tcp":
		addr = cfg.Transport.Address + ":" + cfg.Transport.Port
	case "unix":
		addr = cfg.Transport.Address
	default:
		return nil, fmt.Errorf("unsupported protocol: %s", cfg.Transport.Protocol)
	}

	return grpc.NewClient(addr, options...)
}

func withOutgoingContext(ctx context.Context, method string, req, resp interface{}, cc *grpc.ClientConn, invoker grpc.UnaryInvoker, opts ...grpc.CallOption) error {

	var outmd grpc_metadata.MD
	if md, ok := grpc_metadata.FromOutgoingContext(ctx); ok {
		outmd = md.Copy()
	}

	ctx = grpc_metadata.NewOutgoingContext(context.Background(), outmd)

	return invoker(ctx, method, req, resp, cc, opts...)
}
