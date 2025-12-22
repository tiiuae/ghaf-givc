// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// The GIVC agent is a service that allows remote management of systemd units and applications.
//
// The built-in gRPC server listens for commands from the GIVC admin server (or other instances) and executes
// them on the local system. In order to configure its functionality, it reads environment variables from the
// respective nixosModule configuration.
package main

import (
	"context"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"sync"
	"syscall"

	givc_config "givc/modules/pkgs/config"
	givc_exec "givc/modules/pkgs/exec"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_hwidmanager "givc/modules/pkgs/hwidmanager"
	givc_localelistener "givc/modules/pkgs/localelistener"
	givc_notifier "givc/modules/pkgs/notifier"
	givc_policyagent "givc/modules/pkgs/policyagent"
	givc_registration "givc/modules/pkgs/registration"
	givc_servicemanager "givc/modules/pkgs/servicemanager"
	givc_statsmanager "givc/modules/pkgs/statsmanager"
	givc_types "givc/modules/pkgs/types"
	givc_wifimanager "givc/modules/pkgs/wifimanager"

	log "github.com/sirupsen/logrus"
)

// setupGRPCServices creates and configures all required and optional GRPC services
func setupGRPCServices(agentEndpointConfig *givc_types.EndpointConfig, config *givc_config.AgentConfig) ([]givc_types.GrpcServiceRegistration, *givc_servicemanager.SystemdControlServer, error) {
	var grpcServices []givc_types.GrpcServiceRegistration

	// Systemd control server
	systemdControlServer, err := givc_servicemanager.NewSystemdControlServer(agentEndpointConfig.Services, config.Capabilities.Applications)
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

	// Policy agent server
	if config.Capabilities.Optional.PolicyAgentEnabled {

		log.Infof("policy-agent: agent starting... ")
		policyAgentServer, err := givc_policyagent.NewPolicyAgentServer()
		if err != nil {
			log.Fatalf("policy-agent: cannot create policy agent server: %v", err)
		} else {
			log.Infof("policy-agent: agent started.")
		}
		grpcServices = append(grpcServices, policyAgentServer)
	}

	// Optional capability services - instantiate based on config flags
	if config.Capabilities.Optional.ExecEnabled {
		execServer, err := givc_exec.NewExecServer()
		if err != nil {
			log.Errorf("Cannot create exec server: %v", err)
		} else {
			log.Warnf("Exec capability enabled - allows remote command execution!")
			grpcServices = append(grpcServices, execServer)
		}
	}

	if config.Capabilities.Optional.WifiEnabled {
		wifiServer, err := givc_wifimanager.NewWifiControlServer()
		if err != nil {
			log.Errorf("Cannot create wifi server: %v", err)
		} else {
			log.Infof("WiFi management capability enabled")
			grpcServices = append(grpcServices, wifiServer)
		}
	}

	if config.Capabilities.Optional.HwidEnabled {
		hwidServer, err := givc_hwidmanager.NewHwIdServer(config.Capabilities.Optional.HwidInterface)
		if err != nil {
			log.Errorf("Cannot create hwid server: %v", err)
		} else {
			log.Infof("Hardware ID capability enabled")
			grpcServices = append(grpcServices, hwidServer)
		}
	}

	if config.Capabilities.Optional.NotifierEnabled {
		notifierServer, err := givc_notifier.NewUserNotifierServer(config.Capabilities.Optional.NotifierSocket)
		if err != nil {
			log.Errorf("Cannot create notification server: %v", err)
		} else {
			log.Infof("Notification service capability enabled")
			grpcServices = append(grpcServices, notifierServer)
		}
	}

	return grpcServices, systemdControlServer, nil
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
	if !config.Runtime.Debug {
		log.SetLevel(log.WarnLevel)
	}

	// Endpoint configurations are already created during parsing
	agentEndpointConfig := config.Network.AgentEndpoint
	log.Infof("Allowed systemd units: %v", agentEndpointConfig.Services)

	// Setup GRPC services
	grpcServices, systemdControlServer, err := setupGRPCServices(agentEndpointConfig, config)
	if err != nil {
		log.Errorf("Cannot create GRPC services: %v", err)
		return
	}
	defer systemdControlServer.Close()

	// Start external services
	StartSocketService(ctx, &wg, config)
	StartEventService(ctx, &wg, config)

	// Start agent registration
	serverStarted := make(chan struct{})
	registrationConfig := givc_registration.RegistrationConfig{
		SystemdServer: systemdControlServer,
		AgentConfig:   config,
	}
	registry := givc_registration.NewServiceRegistry(registrationConfig)
	if registry == nil {
		log.Errorf("failed to create service registry")
		return
	}
	registry.StartRegistrationWorker(ctx, &wg, serverStarted)

	// Start main grpc server
	grpcServer, err := givc_grpc.NewServer(agentEndpointConfig, grpcServices)
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
