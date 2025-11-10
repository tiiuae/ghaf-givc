// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// The GIVC agent is a service that allows remote management of systemd units and applications.
//
// The built-in gRPC server listens for commands from the GIVC admin server (or other instances) and executes
// them on the local system. In order to configure its functionality, it reads environment variables from the
// respective nixosModule configuration.
package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"fmt"
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
	givc_exec "givc/modules/pkgs/exec"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_hwidmanager "givc/modules/pkgs/hwidmanager"
	givc_localelistener "givc/modules/pkgs/localelistener"
	givc_serviceclient "givc/modules/pkgs/serviceclient"
	givc_servicemanager "givc/modules/pkgs/servicemanager"
	givc_statsmanager "givc/modules/pkgs/statsmanager"
	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"
	givc_wifimanager "givc/modules/pkgs/wifimanager"

	log "github.com/sirupsen/logrus"
)

func parseJSONEnv(envVar string, target any, required bool) error {
	jsonString, present := os.LookupEnv(envVar)

	if !present || jsonString == "" {
		if required {
			return fmt.Errorf("no '%s' environment variable present", envVar)
		}
		return nil
	}

	err := json.Unmarshal([]byte(jsonString), target)
	if err != nil {
		return fmt.Errorf("error parsing %s JSON: %w", envVar, err)
	}

	return nil
}

func parseAgentType(envVar string) uint32 {
	parsedType, err := strconv.ParseUint(os.Getenv(envVar), 10, 32)
	if err != nil || parsedType > givc_types.UNIT_TYPE_APPVM_APP {
		log.Fatalf("No or wrong '%s' environment variable present.", envVar)
	}
	return uint32(parsedType)
}

func parseServices(agentSubType uint32) map[string]uint32 {
	services := make(map[string]uint32)

	serviceTypes := []struct {
		envVar      string
		serviceType uint32
	}{
		{"SERVICES", agentSubType}, // TODO This should be refactor to UNITS
		{"ADMVMS", givc_types.UNIT_TYPE_ADMVM},
		{"SYSVMS", givc_types.UNIT_TYPE_SYSVM},
		{"APPVMS", givc_types.UNIT_TYPE_APPVM},
	}

	for _, serviceType := range serviceTypes {
		servicesString := os.Getenv(serviceType.envVar)
		if servicesString != "" {
			for service := range strings.FieldsSeq(servicesString) {
				services[service] = serviceType.serviceType
			}
		}
	}

	return services
}

func parseOptionalServices() []givc_types.GrpcServiceRegistration {
	var services []givc_types.GrpcServiceRegistration

	// Parse and create exec server
	execService, execOption := os.LookupEnv("EXEC")
	if execOption && execService != "false" {
		execServer, err := givc_exec.NewExecServer()
		if err != nil {
			log.Fatalf("Cannot create exec server: %v", err)
		}
		log.Warnf("Enabling exec server - this allows remote execution of arbitrary commands!")
		services = append(services, execServer)
	}

	// Parse and create wifi control server
	wifiService, wifiOption := os.LookupEnv("WIFI")
	if wifiOption && wifiService != "false" {
		wifiControlServer, err := givc_wifimanager.NewWifiControlServer()
		if err != nil {
			log.Fatalf("Cannot create wifi control server: %v", err)
		}
		services = append(services, wifiControlServer)
	}

	// Parse and create hwid server
	hwidService, hwidOption := os.LookupEnv("HWID")
	if hwidOption && hwidService != "false" {
		hwidIface := ""
		if _, hwidIfOption := os.LookupEnv("HWID_IFACE"); hwidIfOption {
			hwidIface = os.Getenv("HWID_IFACE")
		}
		hwidServer, err := givc_hwidmanager.NewHwIdServer(hwidIface)
		if err != nil {
			log.Fatalf("Cannot create hwid server: %v", err)
		}
		services = append(services, hwidServer)
	}

	return services
}

type RegistrationConfig struct {
	SystemdServer    *givc_servicemanager.SystemdControlServer
	AdminConfig      *givc_types.EndpointConfig
	AgentConfig      *givc_types.EndpointConfig
	AgentServiceName string
	AgentType        uint32
	AgentParent      string
	Services         map[string]uint32
}

func startRegistrationWorker(ctx context.Context, config RegistrationConfig, serverStarted <-chan struct{}) {
	go func() {
		// Wait for server to start
		select {
		case <-serverStarted:
		case <-ctx.Done():
			log.Infof("Registration cancelled before server start")
			return
		}

		if err := registerAgent(ctx, config); err != nil {
			log.Errorf("Failed to register agent: %v", err)
			return
		}

		registerServices(ctx, config)
	}()
}

func registerAgent(ctx context.Context, config RegistrationConfig) error {
	unitStatus, err := config.SystemdServer.GetUnitStatus(ctx, &givc_systemd.UnitRequest{UnitName: config.AgentServiceName})
	if err != nil {
		return err
	}

	agentEntryRequest := &givc_admin.RegistryRequest{
		Name:   config.AgentServiceName,
		Type:   config.AgentType,
		Parent: config.AgentParent,
		Transport: &givc_admin.TransportConfig{
			Protocol: config.AgentConfig.Transport.Protocol,
			Address:  config.AgentConfig.Transport.Address,
			Port:     config.AgentConfig.Transport.Port,
			Name:     config.AgentConfig.Transport.Name,
		},
		State: unitStatus.UnitStatus,
	}

	// Register agent with admin server with retry loop
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
			_, err = givc_serviceclient.RegisterRemoteService(config.AdminConfig, agentEntryRequest)
			if err == nil {
				log.Infof("Successfully registered agent: %s", config.AgentServiceName)
				return nil
			}
			log.Warnf("Error registering agent: %s, retrying...", err)
			time.Sleep(1 * time.Second)
		}
	}
}

func registerServices(ctx context.Context, config RegistrationConfig) {
	for service, subType := range config.Services {
		if !strings.Contains(service, ".service") {
			continue
		}

		select {
		case <-ctx.Done():
			log.Infof("Service registration cancelled")
			return
		default:
		}

		unitStatus, err := config.SystemdServer.GetUnitStatus(ctx, &givc_systemd.UnitRequest{UnitName: service})
		if err != nil {
			log.Warnf("Error getting unit status for %s: %s", service, err)
			continue
		}

		serviceEntryRequest := &givc_admin.RegistryRequest{
			Name:   service,
			Parent: config.AgentServiceName,
			Type:   uint32(subType),
			Transport: &givc_admin.TransportConfig{
				Name:     config.AgentConfig.Transport.Name,
				Protocol: config.AgentConfig.Transport.Protocol,
				Address:  config.AgentConfig.Transport.Address,
				Port:     config.AgentConfig.Transport.Port,
			},
			State: unitStatus.UnitStatus,
		}

		log.Infof("Trying to register service: %s", service)
		_, err = givc_serviceclient.RegisterRemoteService(config.AdminConfig, serviceEntryRequest)
		if err != nil {
			log.Warnf("Error registering service %s: %s", service, err)
		} else {
			log.Infof("Successfully registered service: %s", service)
		}
	}
}

func main() {

	var err error
	serverStarted := make(chan struct{})

	// Create context for graceful shutdown
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	log.Infof("Running %s", filepath.Base(os.Args[0]))

	// Parse fundamental parameters
	var agent givc_types.TransportConfig
	if err := parseJSONEnv("AGENT", &agent, true); err != nil {
		log.Fatalf("Failed to parse AGENT config: %v", err)
	}

	debug := os.Getenv("DEBUG")
	if debug != "true" {
		log.SetLevel(log.WarnLevel)
	}

	agentType := parseAgentType("TYPE")
	agentSubType := parseAgentType("SUBTYPE")
	agentParent := os.Getenv("PARENT")

	// Configure system services/services/vms to be administrated by this agent
	services := parseServices(agentSubType)

	// Configure applications to be administrated by this agent
	var applications []givc_types.ApplicationManifest
	if err := parseJSONEnv("APPLICATIONS", &applications, false); err != nil {
		log.Fatalf("Failed to parse APPLICATIONS config: %v", err)
	} else if len(applications) > 0 {
		if err := givc_app.ValidateApplicationManifests(applications); err != nil {
			log.Fatalf("Error validating application manifests: %s", err)
		}
	}

	// Set admin server parameters
	var admin givc_types.TransportConfig
	if err := parseJSONEnv("ADMIN_SERVER", &admin, true); err != nil {
		log.Fatalf("Failed to parse ADMIN_SERVER config: %v", err)
	}

	// Configure optional services
	optionalServices := parseOptionalServices()

	// Configure TLS
	var tlsConfigJson givc_types.TlsConfigJson
	if err := parseJSONEnv("TLS_CONFIG", &tlsConfigJson, false); err != nil {
		log.Fatalf("Failed to parse TLS_CONFIG: %v", err)
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

	// Create registration entry
	agentServiceName := "givc-" + agent.Name + ".service"

	// Set agent configurations
	cfgAgent := &givc_types.EndpointConfig{
		Transport: agent,
		TlsConfig: tlsConfig,
		Services:  append([]string{agentServiceName}, slices.Collect(maps.Keys(services))...),
	}

	log.Infof("Allowed systemd units: %v\n", cfgAgent.Services)

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

	// Create statistics server
	statsServer, err := givc_statsmanager.NewStatsServer()
	if err != nil {
		log.Fatalf("Cannot create statistics server: %v", err)
	}
	grpcServices = append(grpcServices, statsServer)

	// Add optional services
	grpcServices = append(grpcServices, optionalServices...)

	// Create socket proxy services
	SetupSocketProxyService(tlsConfig, agent)

	// Create event streaming services
	SetupEventService(tlsConfig)

	// Start registration worker
	registrationConfig := RegistrationConfig{
		SystemdServer:    systemdControlServer,
		AdminConfig:      cfgAdminServer,
		AgentConfig:      cfgAgent,
		AgentServiceName: agentServiceName,
		AgentType:        agentType,
		AgentParent:      agentParent,
		Services:         services,
	}
	startRegistrationWorker(ctx, registrationConfig, serverStarted)

	// Create and start main grpc server
	grpcServer, err := givc_grpc.NewServer(cfgAgent, grpcServices)
	if err != nil {
		log.Fatalf("Cannot create grpc server config: %v", err)
	}
	err = grpcServer.ListenAndServe(ctx, serverStarted)
	if err != nil {
		log.Fatalf("Grpc server failed: %v", err)
	}
}
