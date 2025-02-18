// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"time"

	givc_admin "givc/modules/api/admin"
	givc_systemd "givc/modules/api/systemd"
	givc_app "givc/modules/pkgs/applications"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_hwidmanager "givc/modules/pkgs/hwidmanager"
	givc_localelistener "givc/modules/pkgs/localelistener"
	givc_serviceclient "givc/modules/pkgs/serviceclient"
	givc_servicemanager "givc/modules/pkgs/servicemanager"
	givc_socketproxy "givc/modules/pkgs/socketproxy"
	givc_statsmanager "givc/modules/pkgs/statsmanager"
	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"
	givc_wifimanager "givc/modules/pkgs/wifimanager"

	log "github.com/sirupsen/logrus"
)

func main() {

	var err error
	serverStarted := make(chan struct{})

	log.Infof("Running %s", filepath.Base(os.Args[0]))

	// Parse fundamental parameters
	var agent givc_types.TransportConfig
	jsonTransportConfigString, transportConfigPresent := os.LookupEnv("AGENT")
	if jsonTransportConfigString != "" && transportConfigPresent {
		err := json.Unmarshal([]byte(jsonTransportConfigString), &agent)
		if err != nil {
			log.Fatalf("Error parsing application manifests: %s", err)
		}
	} else {
		log.Fatalf("No 'AGENT' environment variable present.")
	}

	debug := os.Getenv("DEBUG")
	if debug != "true" {
		log.SetLevel(log.WarnLevel)
	}

	parentName := os.Getenv("PARENT")

	parsedType, err := strconv.ParseUint(os.Getenv("TYPE"), 10, 32)
	if err != nil || parsedType > 14 {
		log.Fatalf("No or wrong 'TYPE' environment variable present.")
	}
	agentType := uint32(parsedType)
	parsedType, err = strconv.ParseUint(os.Getenv("SUBTYPE"), 10, 32)
	if err != nil || parsedType > 14 {
		log.Fatalf("No or wrong 'SUBTYPE' environment variable present.")
	}
	agentSubType := uint32(parsedType)

	// Configure system services/units/vms to be administrated by this agent
	services := make(map[string]uint32)
	servicesString := os.Getenv("SERVICES")
	if servicesString != "" {
		servs := strings.Split(servicesString, " ")
		for _, service := range servs {
			services[service] = agentSubType
		}
	}

	sysVmsString := os.Getenv("SYSVMS")
	if sysVmsString != "" {
		sysVms := strings.Split(sysVmsString, " ")
		for _, service := range sysVms {
			services[service] = givc_types.UNIT_TYPE_SYSVM
		}
	}

	appVmsString := os.Getenv("APPVMS")
	if appVmsString != "" {
		appVms := strings.Split(appVmsString, " ")
		for _, service := range appVms {
			services[service] = givc_types.UNIT_TYPE_APPVM
		}
	}

	// Configure applications to be administrated by this agent
	var applications []givc_types.ApplicationManifest
	jsonApplicationString, appPresent := os.LookupEnv("APPLICATIONS")
	if appPresent && jsonApplicationString != "" {
		applications, err = givc_app.ParseApplicationManifests(jsonApplicationString)
		if err != nil {
			log.Fatalf("Error parsing application manifests: %s", err)
		}
	}

	// Set admin server parameters
	var admin givc_types.TransportConfig
	jsonAdminServerString, adminPresent := os.LookupEnv("ADMIN_SERVER")
	if jsonAdminServerString != "" && adminPresent {
		err := json.Unmarshal([]byte(jsonAdminServerString), &admin)
		if err != nil {
			log.Fatalf("Error parsing admin server transport values: %s", err)
		}
	} else {
		log.Fatalf("No 'ADMIN_SERVER' environment variable present.")
	}

	// Configure optional services
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

	var proxyConfigs []givc_types.ProxyConfig
	jsonDbusproxyString, socketProxyOption := os.LookupEnv("SOCKET_PROXY")
	if socketProxyOption && jsonDbusproxyString != "" {
		err = json.Unmarshal([]byte(jsonDbusproxyString), &proxyConfigs)
		if err != nil {
			log.Fatalf("error unmarshalling JSON string: %v", err)
		}
	}

	// Configure TLS
	var tlsConfigJson givc_types.TlsConfigJson
	jsonTlsConfigString, tlsConfigOption := os.LookupEnv("TLS_CONFIG")
	if tlsConfigOption && jsonTlsConfigString != "" {
		err = json.Unmarshal([]byte(jsonTlsConfigString), &tlsConfigJson)
		if err != nil {
			log.Fatalf("error unmarshalling JSON string: %v", err)
		}
	}

	var tlsConfig *tls.Config
	if tlsConfigJson.Enable {
		tlsConfig = givc_util.TlsServerConfig(tlsConfigJson.CaCertPath, tlsConfigJson.CertPath, tlsConfigJson.KeyPath, true)
	}

	// Set admin server configurations
	cfgAdminServer := &givc_types.EndpointConfig{
		Transport: admin,
		TlsConfig: tlsConfig,
	}

	// Create registeration entry
	agentServiceName := "givc-" + agent.Name + ".service"

	// Set agent configurations
	cfgAgent := &givc_types.EndpointConfig{
		Transport: agent,
		TlsConfig: tlsConfig,
		Services:  append([]string{agentServiceName}, slices.Collect(maps.Keys(services))...),
	}

	log.Infof("Allowed systemd units: %v\n", cfgAgent.Services)

	agentEntryRequest := &givc_admin.RegistryRequest{
		Name:   agentServiceName,
		Type:   uint32(agentType),
		Parent: parentName,
		Transport: &givc_admin.TransportConfig{
			Protocol: cfgAgent.Transport.Protocol,
			Address:  cfgAgent.Transport.Address,
			Port:     cfgAgent.Transport.Port,
			Name:     cfgAgent.Transport.Name,
		},
		State: &givc_systemd.UnitStatus{
			Name: agentServiceName,
		},
	}

	// Register this instance
	go func() {
		// Wait for server to start
		<-serverStarted

		// Register agent with admin server
		_, err := givc_serviceclient.RegisterRemoteService(cfgAdminServer, agentEntryRequest)
		for err != nil {
			log.Warnf("Error register agent: %s", err)
			time.Sleep(1 * time.Second)
			_, err = givc_serviceclient.RegisterRemoteService(cfgAdminServer, agentEntryRequest)
		}

		// Register services with admin server
		for service, subType := range services {
			if strings.Contains(service, ".service") {
				serviceEntryRequest := &givc_admin.RegistryRequest{
					Name:   service,
					Parent: agentServiceName,
					Type:   uint32(subType),
					Transport: &givc_admin.TransportConfig{
						Name:     cfgAgent.Transport.Name,
						Protocol: cfgAgent.Transport.Protocol,
						Address:  cfgAgent.Transport.Address,
						Port:     cfgAgent.Transport.Port,
					},
					State: &givc_systemd.UnitStatus{
						Name: service,
					},
				}
				log.Infof("Trying to register service: %s", service)
				_, err := givc_serviceclient.RegisterRemoteService(cfgAdminServer, serviceEntryRequest)
				if err != nil {
					log.Warnf("Error registering service: %s", err)
				}
			}
		}
	}()

	// Create and register gRPC services
	var grpcServices []givc_types.GrpcServiceRegistration

	// Create systemd control server
	systemdControlServer, err := givc_servicemanager.NewSystemdControlServer(cfgAgent.Services, applications)
	if err != nil {
		log.Fatalf("Cannot create systemd control server: %v", err)
	}
	grpcServices = append(grpcServices, systemdControlServer)

	// Create locale listener server
	localeClientServer, err := givc_localelistener.NewLocaleServer()
	if err != nil {
		log.Fatalf("Cannot create locale listener server: %v", err)
	}
	grpcServices = append(grpcServices, localeClientServer)

	// Create wifi control server (optional)
	if wifiEnabled {
		// Create wifi control server
		wifiControlServer, err := givc_wifimanager.NewWifiControlServer()
		if err != nil {
			log.Fatalf("Cannot create wifi control server: %v", err)
		}
		grpcServices = append(grpcServices, wifiControlServer)
	}

	// Create hwid server (optional)
	if hwidEnabled {
		hwidServer, err := givc_hwidmanager.NewHwIdServer(hwidIface)
		if err != nil {
			log.Fatalf("Cannot create hwid server: %v", err)
		}
		grpcServices = append(grpcServices, hwidServer)
	}

	statsServer, err := givc_statsmanager.NewStatsServer()
	if err != nil {
		log.Fatalf("Cannot create statistics server: %v", err)
	}
	grpcServices = append(grpcServices, statsServer)

	// Create socket proxy server (optional)
	for _, proxyConfig := range proxyConfigs {

		// Create socket proxy server for dbus
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
						Name:     cfgAgent.Transport.Name,
						Address:  cfgAgent.Transport.Address,
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

	// Create and start main grpc server
	grpcServer, err := givc_grpc.NewServer(cfgAgent, grpcServices)
	if err != nil {
		log.Fatalf("Cannot create grpc server config: %v", err)
	}
	err = grpcServer.ListenAndServe(context.Background(), serverStarted)
	if err != nil {
		log.Fatalf("Grpc server failed: %v", err)
	}
}
