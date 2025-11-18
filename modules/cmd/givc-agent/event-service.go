// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// This implementation enables remote input device virtualization
// by streaming InputEvents from one VM to another using gRPC.

package main

import (
	"context"
	"strings"
	"sync"

	givc_config "givc/modules/pkgs/config"
	givc_eventproxy "givc/modules/pkgs/eventproxy"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_types "givc/modules/pkgs/types"

	log "github.com/sirupsen/logrus"
)

func StartEventService(ctx context.Context, wg *sync.WaitGroup, config *givc_config.AgentConfig) {

	for _, eventConfig := range config.Network.Bridge.Events {

		eventProxyServer, err := givc_eventproxy.NewEventProxyServer(eventConfig.Transport)
		if err != nil {
			log.Errorf("event: cannot create event proxy server: %v", err)
			continue
		}

		// Setup the event proxy server
		if !eventConfig.Producer {
			log.Infof("event: configuring event proxy server: %v", eventConfig)

			wg.Add(1)
			go func(eventConfig givc_types.EventConfig) {
				defer wg.Done()

				select {
				case <-ctx.Done():
					log.Infof("event: server setup cancelled before start")
					return
				default:
				}

				// Event proxy server config
				cfgEventServer := &givc_types.EndpointConfig{
					Transport: givc_types.TransportConfig{
						Name:     eventConfig.Transport.Name,
						Address:  eventConfig.Transport.Address,
						Port:     eventConfig.Transport.Port,
						Protocol: eventConfig.Transport.Protocol,
					},
					TlsConfig: config.Network.TlsConfig,
				}

				var grpcProxyService []givc_types.GrpcServiceRegistration
				grpcProxyService = append(grpcProxyService, eventProxyServer)
				grpcServer, err := givc_grpc.NewServer(cfgEventServer, grpcProxyService)
				if err != nil {
					log.Errorf("event: cannot create grpc proxy server config: %v", err)
					return
				}
				err = grpcServer.ListenAndServe(ctx, make(chan struct{}))
				if err != nil {
					log.Errorf("event: grpc event server failed: %v", err)
				}
				log.Infof("event: server goroutine finished")

			}(eventConfig)
		} else {

			wg.Add(1)
			go func(eventConfig givc_types.EventConfig) {
				defer wg.Done()

				select {
				case <-ctx.Done():
					log.Infof("event: client setup cancelled before start")
					return
				default:
				}

				// Configure client endpoint
				eventClient := &givc_types.EndpointConfig{
					Transport: eventConfig.Transport,
					TlsConfig: config.Network.TlsConfig,
				}

				err = eventProxyServer.StreamEventsToRemote(ctx, eventClient, eventConfig.Device)
				for err != nil && strings.Contains(err.Error(), "device disconnected") {
					select {
					case <-ctx.Done():
						log.Infof("event: client retry cancelled")
						return
					default:
						log.Errorf("event: retrying to stream events %v", err)
						err = eventProxyServer.StreamEventsToRemote(ctx, eventClient, eventConfig.Device)
					}
				}
				log.Errorf("event: client stream exited: %v", err)
			}(eventConfig)
		}
	}

}
