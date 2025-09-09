// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// This implementation enables remote input device virtualization
// by streaming InputEvents from one VM to another using gRPC.

package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"os"
	"strings"

	givc_eventproxy "givc/modules/pkgs/eventproxy"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_types "givc/modules/pkgs/types"

	log "github.com/sirupsen/logrus"
)

func SetupEventService(tlsConfig *tls.Config) {

	var eventConfigs []givc_types.EventConfig
	var err error
	jsonEventproxyString, eventProxyOption := os.LookupEnv("EVENT_PROXY")
	if eventProxyOption && jsonEventproxyString != "" {
		err = json.Unmarshal([]byte(jsonEventproxyString), &eventConfigs)
		if err != nil {
			log.Fatalf("event: error unmarshalling JSON string: %v", err)
		}
	}

	for _, eventConfig := range eventConfigs {

		eventProxyServer, err := givc_eventproxy.NewEventProxyServer(eventConfig.Transport)
		if err != nil {
			log.Errorf("event: cannot create event proxy server: %v", err)
		}

		// Setup the event proxy server
		if !eventConfig.Producer {
			log.Infof("event: configuring event proxy server: %v", eventConfig)

			go func(eventConfig givc_types.EventConfig) {

				// Event proxy server config
				cfgEventServer := &givc_types.EndpointConfig{
					Transport: givc_types.TransportConfig{
						Name:     eventConfig.Transport.Name,
						Address:  eventConfig.Transport.Address,
						Port:     eventConfig.Transport.Port,
						Protocol: eventConfig.Transport.Protocol,
					},
					TlsConfig: tlsConfig,
				}

				var grpcProxyService []givc_types.GrpcServiceRegistration
				grpcProxyService = append(grpcProxyService, eventProxyServer)
				grpcServer, err := givc_grpc.NewServer(cfgEventServer, grpcProxyService)
				if err != nil {
					log.Errorf("event: cannot create grpc proxy server config: %v", err)
				}
				err = grpcServer.ListenAndServe(context.Background(), make(chan struct{}))
				if err != nil {
					log.Errorf("event: grpc socket proxy server failed: %v", err)
				}

			}(eventConfig)
		} else {

			go func(eventConfig givc_types.EventConfig) {

				// Configure client endpoint
				eventClient := &givc_types.EndpointConfig{
					Transport: eventConfig.Transport,
					TlsConfig: tlsConfig,
				}

				err = eventProxyServer.StreamEventsToRemote(context.Background(), eventClient, eventConfig.Device)
				for err != nil && strings.Contains(err.Error(), "device disconnected") {
					log.Errorf("event: retrying to stream events %v", err)
					err = eventProxyServer.StreamEventsToRemote(context.Background(), eventClient, eventConfig.Device)
				}
				log.Errorf("event: client stream exited: %v", err)
			}(eventConfig)
		}
	}

}
