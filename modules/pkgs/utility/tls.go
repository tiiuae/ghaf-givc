// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package utility

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"net"
	"os"
	"path/filepath"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"
)

var (
	CIPHER_SUITES = []uint16{
		tls.TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
		tls.TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
		tls.TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305,
	}
)

func TlsServerConfig(cacertFilePath string, certFilePath string, keyFilePath string, mutual bool) (*tls.Config, error) {

	// Load TLS certificates and key
	serverTLSCert, err := tls.LoadX509KeyPair(filepath.Clean(certFilePath), filepath.Clean(keyFilePath))
	if err != nil {
		log.Errorf("[TlsServerConfig] Error loading server certificate and key file: %v", err)
		return nil, err
	}
	certPool := x509.NewCertPool()
	caCertPEM, err := os.ReadFile(filepath.Clean(cacertFilePath))
	if err != nil {
		log.Errorf("[TlsServerConfig] Error loading CA certificate: %v", err)
		return nil, err
	}

	ok := certPool.AppendCertsFromPEM(caCertPEM)
	if !ok {
		log.Errorf("[TlsServerConfig] Invalid CA certificate.")
		return nil, fmt.Errorf("invalid CA certificate")
	}

	var clientAuth tls.ClientAuthType
	if mutual {
		clientAuth = tls.RequireAndVerifyClientCert
	} else {
		clientAuth = tls.NoClientCert
	}

	// Set TLS configuration
	tlsConfig := &tls.Config{
		MinVersion:   tls.VersionTLS13,
		ClientAuth:   clientAuth,
		ClientCAs:    certPool,
		RootCAs:      certPool,
		Certificates: []tls.Certificate{serverTLSCert},
		CipherSuites: CIPHER_SUITES,
	}

	return tlsConfig, nil
}

func TlsClientConfig(cacertFilePath string, certFilePath string, keyFilePath string, serverName string) (*tls.Config, error) {

	// Load TLS certificates and key
	clientTLSCert, err := tls.LoadX509KeyPair(certFilePath, keyFilePath)
	if err != nil {
		log.Errorf("[TlsClientConfig] Error loading client certificate and key file: %v", err)
		return nil, err
	}
	certPool := x509.NewCertPool()
	caCertPEM, err := os.ReadFile(filepath.Clean(cacertFilePath))
	if err != nil {
		log.Errorf("[TlsClientConfig] Error loading CA certificate: %v", err)
		return nil, err
	}
	ok := certPool.AppendCertsFromPEM(caCertPEM)
	if !ok {
		log.Errorf("[TlsClientConfig] Invalid CA certificate.")
		return nil, fmt.Errorf("invalid CA certificate")
	}

	// Set TLS configuration
	tlsConfig := &tls.Config{
		MinVersion:   tls.VersionTLS13,
		ServerName:   serverName,
		RootCAs:      certPool,
		Certificates: []tls.Certificate{clientTLSCert},
		CipherSuites: CIPHER_SUITES,
	}

	return tlsConfig, nil
}

func TlsClientConfigFromTlsConfig(tlsConfig *tls.Config, serverName string) (*tls.Config, error) {

	// Return nil if no TLS config is set
	if tlsConfig == nil {
		return nil, fmt.Errorf("no TLS config provided")
	}

	// Set TLS configuration
	newTlsConfig := &tls.Config{
		MinVersion:   tls.VersionTLS13,
		ServerName:   serverName,
		RootCAs:      tlsConfig.RootCAs,
		Certificates: tlsConfig.Certificates,
		CipherSuites: CIPHER_SUITES,
	}

	return newTlsConfig, nil
}

// CertIPVerifyInterceptor is a gRPC server interceptor that verifies
// the peer's IP address matches an IP in their TLS certificate's SubjectAltName.
//
// TCP: Verifies peer IP matches certificate SAN.
// Vsock/Unix: Skips IP check (hypervisor/filesystem provides isolation).
func CertIPVerifyInterceptor(ctx context.Context, req any,
	info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {

	// Extract peer info from context
	p, ok := peer.FromContext(ctx)
	if !ok {
		return nil, status.Error(codes.Unauthenticated, "no peer info available")
	}

	// Get TLS info from peer
	tlsInfo, ok := p.AuthInfo.(credentials.TLSInfo)
	if !ok {
		// No TLS - skip verification
		return handler(ctx, req)
	}

	// Get peer certificate
	if len(tlsInfo.State.PeerCertificates) == 0 {
		return nil, status.Error(codes.Unauthenticated, "no peer certificate provided")
	}

	// Skip IP verification for non-TCP transports
	network := p.Addr.Network()
	if network == "vsock" || network == "unix" {
		log.Debugf("[CertIPVerifyInterceptor] %s transport, skipping IP check", network)
		return handler(ctx, req)
	}

	cert := tlsInfo.State.PeerCertificates[0]

	// Extract peer IP from connection address
	peerIP, err := extractIPFromAddr(p.Addr)
	if err != nil {
		log.Errorf("[CertIPVerifyInterceptor] Failed to extract peer IP: %v", err)
		return nil, status.Errorf(codes.Unauthenticated, "cannot determine peer IP: %v", err)
	}

	// Verify peer IP is in certificate's SubjectAltName
	if !ipInCertSAN(cert, peerIP) {
		log.Warnf("[CertIPVerifyInterceptor] IP verification failed: peer IP %s not in certificate SAN IPs %v",
			peerIP, cert.IPAddresses)
		return nil, status.Errorf(codes.PermissionDenied,
			"peer IP %s does not match any IP in certificate", peerIP)
	}

	log.Debugf("[CertIPVerifyInterceptor] IP verification passed for %s", peerIP)
	return handler(ctx, req)
}

// extractIPFromAddr extracts the IP address from a net.Addr
func extractIPFromAddr(addr net.Addr) (net.IP, error) {
	if addr == nil {
		return nil, fmt.Errorf("nil address")
	}

	switch a := addr.(type) {
	case *net.TCPAddr:
		return a.IP, nil
	case *net.UDPAddr:
		return a.IP, nil
	default:
		// Try to parse as host:port string
		host, _, err := net.SplitHostPort(addr.String())
		if err != nil {
			if ip := net.ParseIP(addr.String()); ip != nil {
				return ip, nil
			}
			return nil, fmt.Errorf("cannot parse address %q: %v", addr.String(), err)
		}
		ip := net.ParseIP(host)
		if ip == nil {
			return nil, fmt.Errorf("cannot parse IP from %q", host)
		}
		return ip, nil
	}
}

// ipInCertSAN checks if the given IP is in the certificate's SubjectAltName IP addresses
func ipInCertSAN(cert *x509.Certificate, ip net.IP) bool {
	for _, certIP := range cert.IPAddresses {
		if certIP.Equal(ip) {
			return true
		}
	}
	return false
}
