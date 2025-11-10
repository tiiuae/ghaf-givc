// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// This implementation enables socket proxy functionality
// by streaming socket connections from one location to another using gRPC.

package main

import (
	"context"
	"sync"

	givc_grpc "givc/modules/pkgs/grpc"
	givc_socketproxy "givc/modules/pkgs/socketproxy"
	givc_types "givc/modules/pkgs/types"

	log "github.com/sirupsen/logrus"
)

func StartSocketProxyService(ctx context.Context, wg *sync.WaitGroup, agentConfig *AgentConfig) {

	for _, proxyConfig := range agentConfig.ProxyConfigs {

		// Create socket proxy server
		socketProxyServer, err := givc_socketproxy.NewSocketProxyServer(proxyConfig.Socket, proxyConfig.Server)
		if err != nil {
			log.Errorf("Cannot create socket proxy server: %v", err)
			continue
		}

		// Run proxy client
		if !proxyConfig.Server {
			log.Infof("Configuring socket proxy client: %v", proxyConfig)

			wg.Add(1)
			go func(proxyConfig givc_types.ProxyConfig) {
				defer wg.Done()

				select {
				case <-ctx.Done():
					log.Infof("socket-proxy: client setup cancelled before start")
					return
				default:
				}

				// Configure client endpoint
				socketClient := &givc_types.EndpointConfig{
					Transport: proxyConfig.Transport,
					TlsConfig: agentConfig.TlsConfig,
				}

				err = socketProxyServer.StreamToRemote(ctx, socketClient)
				if err != nil {
					log.Errorf("Socket client stream exited: %v", err)
				}
				log.Infof("socket-proxy: client goroutine finished")

			}(proxyConfig)
		}

		// Run proxy server
		if proxyConfig.Server {
			log.Infof("Configuring socket proxy server: %v", proxyConfig)

			wg.Add(1)
			go func(proxyConfig givc_types.ProxyConfig) {
				defer wg.Done()

				select {
				case <-ctx.Done():
					log.Infof("socket-proxy: server setup cancelled before start")
					return
				default:
				}

				// Socket proxy server config
				cfgProxyServer := &givc_types.EndpointConfig{
					Transport: givc_types.TransportConfig{
						Name:     agentConfig.Agent.Name,
						Address:  agentConfig.Agent.Address,
						Port:     proxyConfig.Transport.Port,
						Protocol: proxyConfig.Transport.Protocol,
					},
					TlsConfig: agentConfig.TlsConfig,
				}

				var grpcProxyService []givc_types.GrpcServiceRegistration
				grpcProxyService = append(grpcProxyService, socketProxyServer)
				grpcServer, err := givc_grpc.NewServer(cfgProxyServer, grpcProxyService)
				if err != nil {
					log.Errorf("Cannot create grpc proxy server config: %v", err)
					return
				}
				err = grpcServer.ListenAndServe(ctx, make(chan struct{}))
				if err != nil {
					log.Errorf("Grpc socket proxy server failed: %v", err)
				}
				log.Infof("socket-proxy: server goroutine finished")

			}(proxyConfig)
		}
	}
}
