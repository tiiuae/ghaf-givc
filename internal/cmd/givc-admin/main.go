// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package main

import (
	"context"
	"os"
	"strings"
	"time"

	givc_grpc "givc/internal/pkgs/grpc"
	"givc/internal/pkgs/systemmanager"
	"givc/internal/pkgs/types"
	givc_util "givc/internal/pkgs/utility"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type GrpcServiceRegistration interface {
	Name() string
	RegisterService(*grpc.Server)
}

var (
	cfgServer = types.EndpointConfig{
		Transport: types.TransportConfig{
			Name:     "localhost",
			Address:  "127.0.0.1",
			Port:     "9000",
			Protocol: "tcp",
		},
		TlsConfig: nil,
	}
)

func main() {

	log.Infof("Executing %s \n", os.Args[0])

	name := os.Getenv("NAME")
	if name == "" {
		log.Fatalf("No 'NAME' environment variable present.")
	}
	cfgServer.Transport.Name = name

	address := os.Getenv("ADDR")
	if address == "" {
		log.Fatalf("No 'ADDR' environment variable present.")
	}
	cfgServer.Transport.Address = address

	port := os.Getenv("PORT")
	if port == "" {
		log.Fatalf("No 'PORT' environment variable present.")
	}
	cfgServer.Transport.Port = port

	protocol := os.Getenv("PROTO")
	if protocol == "" {
		log.Fatalf("No 'PROTO' environment variable present.")
	}
	cfgServer.Transport.Protocol = protocol

	services := strings.Split(os.Getenv("SERVICES"), " ")
	if len(services) < 1 {
		log.Fatalf("A space-separated list of services (host and system-vms) is required in environment variable $SERVICES.")
	}
	cfgServer.Services = append(cfgServer.Services, services...)
	log.Infof("Required services: %v\n", cfgServer.Services)

	if os.Getenv("TLS") != "false" {
		cacert := os.Getenv("CA_CERT")
		if cacert == "" {
			log.Fatalf("No 'CA_CERT' environment variable present. To turn off TLS set 'TLS' to 'false'.")
		}
		cert := os.Getenv("HOST_CERT")
		if cert == "" {
			log.Fatalf("No 'HOST_CERT' environment variable present. To turn off TLS set 'TLS' to 'false'.")
		}
		key := os.Getenv("HOST_KEY")
		if key == "" {
			log.Fatalf("No 'HOST_KEY' environment variable present. To turn off TLS set 'TLS' to 'false'.")
		}
		// @TODO add path and file checks
		cfgServer.TlsConfig = givc_util.TlsServerConfig(cacert, cert, key, true)
	}

	// Create admin server
	adminServer := systemmanager.NewAdminServer(&cfgServer)

	// Start monitoring
	go func() {
		time.Sleep(4 * time.Second)
		adminServer.AdminService.Monitor()
	}()

	// Start server
	grpcServer, err := givc_grpc.NewServer(&cfgServer, []types.GrpcServiceRegistration{adminServer})
	if err != nil {
		log.Fatalf("Cannot create grpc server config")
	}

	ctx := context.Background()
	err = grpcServer.ListenAndServe(ctx)
	if err != nil {
		log.Fatalf("Grpc server failed: %s", err)
	}
}
