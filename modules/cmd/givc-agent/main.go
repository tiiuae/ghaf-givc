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
	"fmt"
	"maps"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"slices"
	"sync"
	"syscall"

	givc_config "givc/modules/pkgs/config"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_localelistener "givc/modules/pkgs/localelistener"
	givc_registration "givc/modules/pkgs/registration"
	givc_servicemanager "givc/modules/pkgs/servicemanager"
	givc_statsmanager "givc/modules/pkgs/statsmanager"
	givc_types "givc/modules/pkgs/types"

	log "github.com/sirupsen/logrus"
)

// createEndpointConfigs creates the admin and agent endpoint configurations
func createEndpointConfigs(config *givc_config.AgentConfig) (*givc_types.EndpointConfig, *givc_types.EndpointConfig, string) {
	// Set admin server config
	cfgAdminServer := &givc_types.EndpointConfig{
		Transport: config.Admin,
		TlsConfig: config.TlsConfig,
	}

	// Create endpoint config
	agentServiceName := "givc-" + config.Agent.Name + ".service"
	cfgAgent := &givc_types.EndpointConfig{
		Transport: config.Agent,
		TlsConfig: config.TlsConfig,
		Services:  append([]string{agentServiceName}, slices.Collect(maps.Keys(config.Units))...),
	}

	log.Infof("Allowed systemd units: %v", cfgAgent.Services)

	return cfgAdminServer, cfgAgent, agentServiceName
}

// setupGRPCServices creates and configures all required and optional GRPC services
func setupGRPCServices(cfgAgent *givc_types.EndpointConfig, config *givc_config.AgentConfig) ([]givc_types.GrpcServiceRegistration, *givc_servicemanager.SystemdControlServer, error) {
	var grpcServices []givc_types.GrpcServiceRegistration

	// Systemd control server
	systemdControlServer, err := givc_servicemanager.NewSystemdControlServer(cfgAgent.Services, config.Applications)
	if err != nil {
		return nil, nil, err
	}
	grpcServices = append(grpcServices, systemdControlServer)

	// Locale listener server
	localeClientServer, err := givc_localelistener.NewLocaleServer()
	if err != nil {
		return nil, nil, err
	}
	grpcServices = append(grpcServices, localeClientServer)

	// Statistics server
	statsServer, err := givc_statsmanager.NewStatsServer()
	if err != nil {
		return nil, nil, err
	}
	grpcServices = append(grpcServices, statsServer)

	// OPTIONAL GRPC services
	grpcServices = append(grpcServices, config.OptionalServices...)

	return grpcServices, systemdControlServer, nil
}

// startExternalServices starts separate grpc servers
func startExternalServices(ctx context.Context, wg *sync.WaitGroup, config *givc_config.AgentConfig) {
	StartSocketProxyService(ctx, wg, config)
	StartEventService(ctx, wg, config)
}

// startRegistration configures and starts the registration worker
func startRegistration(ctx context.Context, wg *sync.WaitGroup, registrationConfig givc_registration.RegistrationConfig, serverStarted chan struct{}) error {

	registry := givc_registration.NewServiceRegistry(registrationConfig)
	if registry == nil {
		return fmt.Errorf("failed to create service registry")
	}
	registry.StartRegistrationWorker(ctx, wg, serverStarted)

	return nil
}

// Main function of the GIVC agent.
func main() {

	log.Infof("Running %s", filepath.Base(os.Args[0]))
	exitCode := 1 // Default exit code in case of failure

	// Setup context
	ctx, cancel := context.WithCancel(context.Background())

	// Setup WaitGroup to track background goroutines
	var wg sync.WaitGroup
	defer func() {
		cancel()
		wg.Wait()
		log.Infof("Shutdown complete")
		log.Debugf("final # goroutines: %d", runtime.NumGoroutine())
		os.Exit(exitCode)
	}()

	// Setup shutdown signal handling
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGTERM, syscall.SIGINT)

	wg.Add(1)
	go func() {
		defer wg.Done()
		select {
		case <-ctx.Done():
			return
		case sig := <-sigChan:
			log.Infof("Received signal %v, initiating shutdown", sig)
			exitCode = 0
			cancel()
		}
	}()

	// Parse configuration
	config, err := givc_config.ParseConfig()
	if err != nil {
		log.Errorf("Failed to parse configuration: %v", err)
		return
	}

	// Setup log level
	if !config.Debug {
		log.SetLevel(log.WarnLevel)
	}

	// Setup endpoint configurations
	cfgAdminServer, cfgAgent, agentServiceName := createEndpointConfigs(config)

	// Setup GRPC services
	grpcServices, systemdControlServer, err := setupGRPCServices(cfgAgent, config)
	if err != nil {
		log.Errorf("Cannot create GRPC services: %v", err)
		return
	}
	defer systemdControlServer.Close()

	// Start external services
	startExternalServices(ctx, &wg, config)

	// Start agent registration
	serverStarted := make(chan struct{})
	registrationConfig := givc_registration.RegistrationConfig{
		SystemdServer:    systemdControlServer,
		AdminConfig:      cfgAdminServer,
		AgentConfig:      cfgAgent,
		AgentServiceName: agentServiceName,
		AgentType:        config.AgentType,
		AgentParent:      config.AgentParent,
		Services:         config.Units,
	}
	startRegistration(ctx, &wg, registrationConfig, serverStarted)

	// Start main grpc server
	grpcServer, err := givc_grpc.NewServer(cfgAgent, grpcServices)
	if err != nil {
		log.Errorf("Cannot create grpc server config: %v", err)
		return
	}
	err = grpcServer.ListenAndServe(ctx, serverStarted)
	if err != nil {
		log.Errorf("Grpc server failed: %v", err)
		return
	}
}
