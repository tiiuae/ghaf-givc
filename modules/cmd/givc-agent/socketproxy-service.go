// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// This implementation enables socket proxy functionality
// by streaming socket connections from one location to another using gRPC.

package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"os"

	givc_grpc "givc/modules/pkgs/grpc"
	givc_socketproxy "givc/modules/pkgs/socketproxy"
	givc_types "givc/modules/pkgs/types"

	log "github.com/sirupsen/logrus"
)

func SetupSocketProxyService(tlsConfig *tls.Config, agentTransport givc_types.TransportConfig) {

	var err error

	// Parse socket proxy configuration
	var proxyConfigs []givc_types.ProxyConfig
	jsonDbusproxyString, socketProxyOption := os.LookupEnv("SOCKET_PROXY")
	if socketProxyOption && jsonDbusproxyString != "" {
		err = json.Unmarshal([]byte(jsonDbusproxyString), &proxyConfigs)
		if err != nil {
			log.Fatalf("error unmarshalling JSON string: %v", err)
		}
	}

	// Configure socket proxies
	for _, proxyConfig := range proxyConfigs {

		// Create socket proxy server
		socketProxyServer, err := givc_socketproxy.NewSocketProxyServer(proxyConfig.Socket, proxyConfig.Server)
		if err != nil {
			log.Errorf("Cannot create socket proxy server: %v", err)
		}

		// Run proxy client
		if !proxyConfig.Server {
			log.Infof("Configuring socket proxy client: %v", proxyConfig)

			go func(proxyConfig givc_types.ProxyConfig) {

				// Configure client endpoint
				socketClient := &givc_types.EndpointConfig{
					Transport: proxyConfig.Transport,
					TlsConfig: tlsConfig,
				}

				err = socketProxyServer.StreamToRemote(context.Background(), socketClient)
				if err != nil {
					log.Errorf("Socket client stream exited: %v", err)
				}

			}(proxyConfig)
		}

		// Run proxy server
		if proxyConfig.Server {
			log.Infof("Configuring socket proxy server: %v", proxyConfig)

			go func(proxyConfig givc_types.ProxyConfig) {

				// Socket proxy server config
				cfgProxyServer := &givc_types.EndpointConfig{
					Transport: givc_types.TransportConfig{
						Name:     agentTransport.Name,
						Address:  agentTransport.Address,
						Port:     proxyConfig.Transport.Port,
						Protocol: proxyConfig.Transport.Protocol,
					},
					TlsConfig: tlsConfig,
				}

				var grpcProxyService []givc_types.GrpcServiceRegistration
				grpcProxyService = append(grpcProxyService, socketProxyServer)
				grpcServer, err := givc_grpc.NewServer(cfgProxyServer, grpcProxyService)
				if err != nil {
					log.Errorf("Cannot create grpc proxy server config: %v", err)
				}
				err = grpcServer.ListenAndServe(context.Background(), make(chan struct{}))
				if err != nil {
					log.Errorf("Grpc socket proxy server failed: %v", err)
				}

			}(proxyConfig)
		}
	}
}
