// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"givc/api/admin"
	givc_grpc "givc/internal/pkgs/grpc"
	"givc/internal/pkgs/hwidmanager"
	"givc/internal/pkgs/serviceclient"
	"givc/internal/pkgs/servicemanager"
	"givc/internal/pkgs/types"
	givc_util "givc/internal/pkgs/utility"
	"givc/internal/pkgs/wifimanager"

	log "github.com/sirupsen/logrus"
)

func main() {

	var err error
	log.Infof("Executing %s", filepath.Base(os.Args[0]))

	name := os.Getenv("NAME")
	if name == "" {
		log.Fatalf("No 'NAME' environment variable present.")
	}
	address := os.Getenv("ADDR")
	if address == "" {
		log.Fatalf("No 'ADDR' environment variable present.")
	}
	port := os.Getenv("PORT")
	if port == "" {
		log.Fatalf("No 'PORT' environment variable present.")
	}
	protocol := os.Getenv("PROTO")
	if protocol == "" {
		log.Fatalf("No 'PROTO' environment variable present.")
	}
	debug := os.Getenv("DEBUG")
	if debug != "true" {
		log.SetLevel(log.WarnLevel)
	}

	parentName := os.Getenv("PARENT")

	agentType, err := strconv.ParseUint(os.Getenv("TYPE"), 10, 32)
	if err != nil || agentType > 14 {
		log.Fatalf("No or wrong 'TYPE' environment variable present.")
	}
	agentSubType, err := strconv.ParseUint(os.Getenv("SUBTYPE"), 10, 32)
	if err != nil || agentSubType > 14 {
		log.Fatalf("No or wrong 'SUBTYPE' environment variable present.")
	}

	var services []string
	servicesString, servicesPresent := os.LookupEnv("SERVICES")
	if servicesPresent {
		services = strings.Split(servicesString, " ")
	}
	var applications map[string]string
	jsonApplicationString, appPresent := os.LookupEnv("APPLICATIONS")
	if appPresent && jsonApplicationString != "" {
		applications = make(map[string]string)
		err := json.Unmarshal([]byte(jsonApplicationString), &applications)
		if err != nil {
			log.Fatalf("Error unmarshalling JSON string.")
		}
	}
	adminServerName := os.Getenv("ADMIN_SERVER_NAME")
	if adminServerName == "" {
		log.Fatalf("A name for the admin server is required in environment variable $ADMIN_SERVER_NAME.")
	}
	adminServerAddr := os.Getenv("ADMIN_SERVER_ADDR")
	if adminServerAddr == "" {
		log.Fatalf("An address for the admin server is required in environment variable $ADMIN_SERVER_ADDR.")
	}
	adminServerPort := os.Getenv("ADMIN_SERVER_PORT")
	if adminServerPort == "" {
		log.Fatalf("An port address for the admin server is required in environment variable $ADMIN_SERVER_PORT.")
	}
	adminServerProtocol := os.Getenv("ADMIN_SERVER_PROTO")
	if adminServerProtocol == "" {
		log.Fatalf("An address for the admin server is required in environment variable $ADMIN_SERVER_PROTO.")
	}
	wifiEnabled := false
	wifiService, wifiOption := os.LookupEnv("WIFI")
	if wifiOption && (wifiService != "false") {
		wifiEnabled = true
	}

	hwidEnabled := false
	hwidService, hwidOption := os.LookupEnv("HWID")
	hwidIface, hwidIfOption := os.LookupEnv("HWID_IFACE")
	if hwidOption && (hwidService != "false") {
		if !hwidIfOption {
			hwidIface = ""
		}
		hwidEnabled = true
	}
	var tlsConfig *tls.Config
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
		tlsConfig = givc_util.TlsServerConfig(cacert, cert, key, true)
	}
	// @TODO add path and file checks

	cfgAdminServer := &types.EndpointConfig{
		Transport: types.TransportConfig{
			Name:     adminServerName,
			Address:  adminServerAddr,
			Port:     adminServerPort,
			Protocol: adminServerProtocol,
		},
		TlsConfig: tlsConfig,
	}

	// Set agent configurations
	cfgAgent := &types.EndpointConfig{
		Transport: types.TransportConfig{
			Name:     name,
			Address:  address,
			Port:     port,
			Protocol: protocol,
		},
		TlsConfig: tlsConfig,
	}
	agentServiceName := "givc-" + name + ".service"

	// Add services
	cfgAgent.Services = append(cfgAgent.Services, agentServiceName)
	if servicesPresent {
		cfgAgent.Services = append(cfgAgent.Services, services...)
	}
	log.Infof("Started with services: %v\n", cfgAgent.Services)

	agentEntryRequest := &admin.RegistryRequest{
		Name:   agentServiceName,
		Type:   uint32(agentType),
		Parent: parentName,
		Transport: &admin.TransportConfig{
			Protocol: cfgAgent.Transport.Protocol,
			Address:  cfgAgent.Transport.Address,
			Port:     cfgAgent.Transport.Port,
			Name:     cfgAgent.Transport.Name,
		},
		State: &admin.UnitStatus{
			Name: agentServiceName,
		},
	}

	// Register this instance
	serverStarted := make(chan struct{})
	go func() {
		// Wait for server to start
		<-serverStarted

		// Register agent
		_, err := serviceclient.RegisterRemoteService(cfgAdminServer, agentEntryRequest)
		if err != nil {
			log.Fatalf("Error register agent: %s", err)
		}

		// Register services
		for _, service := range services {
			if strings.Contains(service, ".service") {
				serviceEntryRequest := &admin.RegistryRequest{
					Name:   service,
					Parent: agentServiceName,
					Type:   uint32(agentSubType),
					Transport: &admin.TransportConfig{
						Name:     cfgAgent.Transport.Name,
						Protocol: cfgAgent.Transport.Protocol,
						Address:  cfgAgent.Transport.Address,
						Port:     cfgAgent.Transport.Port,
					},
					State: &admin.UnitStatus{
						Name: service,
					},
				}
				log.Infof("Trying to register service: %s", service)
				_, err := serviceclient.RegisterRemoteService(cfgAdminServer, serviceEntryRequest)
				if err != nil {
					log.Warnf("Error registering service: %s", err)
				}
			}
		}
	}()

	// Create and resgister gRPC services
	var grpcServices []types.GrpcServiceRegistration

	// Create systemd control server
	systemdControlServer, err := servicemanager.NewSystemdControlServer(cfgAgent.Services, applications)
	if err != nil {
		log.Fatalf("Cannot create systemd control server")
	}
	grpcServices = append(grpcServices, systemdControlServer)

	if wifiEnabled {
		// Create wifi control server
		wifiControlServer, err := wifimanager.NewWifiControlServer()
		if err != nil {
			log.Fatalf("Cannot create wifi control server")
		}
		grpcServices = append(grpcServices, wifiControlServer)
	}

	if hwidEnabled {
		hwidServer, err := hwidmanager.NewHwIdServer(hwidIface)
		if err != nil {
			log.Fatalf("Cannot create hwid server")
		}
		grpcServices = append(grpcServices, hwidServer)
	}

	// Create grpc server
	grpcServer, err := givc_grpc.NewServer(cfgAgent, grpcServices)
	if err != nil {
		log.Fatalf("Cannot create grpc server config")
	}

	// Start server
	ctx := context.Background()
	err = grpcServer.ListenAndServe(ctx, serverStarted)
	if err != nil {
		log.Fatalf("Grpc server failed: %s", err)
	}
}
