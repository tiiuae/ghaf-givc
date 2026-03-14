// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package grpc

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"os"
	"path/filepath"

	"github.com/spiffe/go-spiffe/v2/spiffeid"
	"github.com/spiffe/go-spiffe/v2/spiffetls/tlsconfig"
	"github.com/spiffe/go-spiffe/v2/workloadapi"
	"google.golang.org/grpc"
	grpc_creds "google.golang.org/grpc/credentials"
)

type StaticTLSProvider struct {
	CACertPath string
	CertPath   string
	KeyPath    string
}

func (s *StaticTLSProvider) ServerOption() (grpc.ServerOption, error) {
	cert, err := tls.LoadX509KeyPair(filepath.Clean(s.CertPath), filepath.Clean(s.KeyPath))
	if err != nil {
		return nil, fmt.Errorf("load server keypair: %w", err)
	}

	caCert, err := os.ReadFile(filepath.Clean(s.CACertPath))
	if err != nil {
		return nil, fmt.Errorf("read CA cert: %w", err)
	}

	certPool := x509.NewCertPool()
	if !certPool.AppendCertsFromPEM(caCert) {
		return nil, fmt.Errorf("invalid CA certificate")
	}

	tlsCfg := &tls.Config{
		Certificates: []tls.Certificate{cert},
		ClientAuth:   tls.RequireAndVerifyClientCert,
		ClientCAs:    certPool,
		MinVersion:   tls.VersionTLS13,
	}

	return grpc.Creds(grpc_creds.NewTLS(tlsCfg)), nil
}

func (s *StaticTLSProvider) ClientDialOption(serverName string) (grpc.DialOption, error) {
	cert, err := tls.LoadX509KeyPair(filepath.Clean(s.CertPath), filepath.Clean(s.KeyPath))
	if err != nil {
		return nil, fmt.Errorf("load client keypair: %w", err)
	}

	caCert, err := os.ReadFile(filepath.Clean(s.CACertPath))
	if err != nil {
		return nil, fmt.Errorf("read CA cert: %w", err)
	}

	certPool := x509.NewCertPool()
	if !certPool.AppendCertsFromPEM(caCert) {
		return nil, fmt.Errorf("invalid CA certificate")
	}

	tlsCfg := &tls.Config{
		Certificates: []tls.Certificate{cert},
		RootCAs:      certPool,
		ServerName:   serverName,
		MinVersion:   tls.VersionTLS13,
	}

	return grpc.WithTransportCredentials(grpc_creds.NewTLS(tlsCfg)), nil
}

func (s *StaticTLSProvider) Close() error { return nil }

type SpiffeTLSConfig struct {
	SocketPath  string
	AllowedIDs  []string
	TrustDomain string
}

type SpiffeTLSProvider struct {
	source     *workloadapi.X509Source
	authorizer tlsconfig.Authorizer
}

func NewSpiffeTLSProvider(cfg SpiffeTLSConfig) (*SpiffeTLSProvider, error) {
	var opts []workloadapi.X509SourceOption
	if cfg.SocketPath != "" {
		opts = append(opts, workloadapi.WithClientOptions(workloadapi.WithAddr(cfg.SocketPath)))
	}

	source, err := workloadapi.NewX509Source(context.Background(), opts...)
	if err != nil {
		return nil, fmt.Errorf("create X509 source: %w", err)
	}

	authorizer, err := buildSpiffeAuthorizer(cfg)
	if err != nil {
		source.Close()
		return nil, err
	}

	return &SpiffeTLSProvider{source: source, authorizer: authorizer}, nil
}

func (sp *SpiffeTLSProvider) ServerOption() (grpc.ServerOption, error) {
	tlsCfg := tlsconfig.MTLSServerConfig(sp.source, sp.source, sp.authorizer)
	tlsCfg.MinVersion = tls.VersionTLS13
	return grpc.Creds(grpc_creds.NewTLS(tlsCfg)), nil
}

func (sp *SpiffeTLSProvider) ClientDialOption(_ string) (grpc.DialOption, error) {
	tlsCfg := tlsconfig.MTLSClientConfig(sp.source, sp.source, sp.authorizer)
	tlsCfg.MinVersion = tls.VersionTLS13
	return grpc.WithTransportCredentials(grpc_creds.NewTLS(tlsCfg)), nil
}

func (sp *SpiffeTLSProvider) Close() error {
	if sp.source == nil {
		return nil
	}
	return sp.source.Close()
}

func (sp *SpiffeTLSProvider) SkipCertIPVerification() bool {
	return true
}

func buildSpiffeAuthorizer(cfg SpiffeTLSConfig) (tlsconfig.Authorizer, error) {
	if len(cfg.AllowedIDs) > 0 {
		ids := make([]spiffeid.ID, 0, len(cfg.AllowedIDs))
		for _, raw := range cfg.AllowedIDs {
			id, err := spiffeid.FromString(raw)
			if err != nil {
				return nil, fmt.Errorf("invalid SPIFFE ID %q: %w", raw, err)
			}
			ids = append(ids, id)
		}
		return tlsconfig.AuthorizeOneOf(ids...), nil
	}

	if cfg.TrustDomain != "" {
		td, err := spiffeid.TrustDomainFromString(cfg.TrustDomain)
		if err != nil {
			return nil, fmt.Errorf("invalid trust domain %q: %w", cfg.TrustDomain, err)
		}
		return tlsconfig.AuthorizeMemberOf(td), nil
	}

	return tlsconfig.AuthorizeAny(), nil
}
