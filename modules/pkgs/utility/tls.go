// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package utility

import (
	"crypto/tls"
	"crypto/x509"
	"fmt"
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
