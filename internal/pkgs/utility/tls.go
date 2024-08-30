// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package utility

import (
	"crypto/tls"
	"crypto/x509"
	"os"
	"path/filepath"

	log "github.com/sirupsen/logrus"
)

var (
	CIPHER_SUITES = []uint16{
		tls.TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
		tls.TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
		tls.TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305,
	}
)

func TlsServerConfig(CACertFilePath string, CertFilePath string, KeyFilePath string, mutual bool) *tls.Config {

	// Load TLS certificates and key
	serverTLSCert, err := tls.LoadX509KeyPair(filepath.Clean(CertFilePath), filepath.Clean(KeyFilePath))
	if err != nil {
		log.Fatalf("[TlsServerConfig] Error loading server certificate and key file: %v", err)
	}
	certPool := x509.NewCertPool()
	caCertPEM, err := os.ReadFile(filepath.Clean(CACertFilePath))
	if err != nil {
		log.Fatalf("[TlsServerConfig] Error loading CA certificate: %v", err)
	}
	ok := certPool.AppendCertsFromPEM(caCertPEM)
	if !ok {
		log.Fatalf("[TlsServerConfig] Invalid CA certificate.")
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

	return tlsConfig
}

func TlsClientConfig(CACertFilePath string, CertFilePath string, KeyFilePath string, serverName string) *tls.Config {

	// Load TLS certificates and key
	clientTLSCert, err := tls.LoadX509KeyPair(CertFilePath, KeyFilePath)
	if err != nil {
		log.Fatalf("[TlsClientConfig] Error loading client certificate and key file: %v", err)
	}
	certPool := x509.NewCertPool()
	caCertPEM, err := os.ReadFile(filepath.Clean(CACertFilePath))
	if err != nil {
		log.Fatalf("[TlsClientConfig] Error loading CA certificate: %v", err)
	}
	ok := certPool.AppendCertsFromPEM(caCertPEM)
	if !ok {
		log.Fatalf("[TlsClientConfig] Invalid CA certificate.")
	}

	// Set TLS configuration
	tlsConfig := &tls.Config{
		MinVersion:   tls.VersionTLS13,
		ServerName:   serverName,
		RootCAs:      certPool,
		Certificates: []tls.Certificate{clientTLSCert},
		CipherSuites: CIPHER_SUITES,
	}

	return tlsConfig
}

func TlsClientConfigFromTlsConfig(tlsConfig *tls.Config, serverName string) *tls.Config {

	// Return nil if no TLS config is set
	if tlsConfig == nil {
		return nil
	}

	// Set TLS configuration
	newTlsConfig := &tls.Config{
		MinVersion:   tls.VersionTLS13,
		ServerName:   serverName,
		RootCAs:      tlsConfig.RootCAs,
		Certificates: tlsConfig.Certificates,
		CipherSuites: CIPHER_SUITES,
	}

	return newTlsConfig
}
