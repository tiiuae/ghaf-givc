// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// The GIVC agent is a service that allows remote management of systemd units and applications.
//
// The built-in gRPC server listens for commands from the GIVC admin server (or other instances) and executes
// them on the local system. Configuration is loaded from a JSON file specified via command-line arguments.
package main

import (
	"context"
	"flag"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"sync"
	"syscall"

	givc_config "givc/modules/pkgs/config"
	givc_ctap "givc/modules/pkgs/ctap"
	givc_exec "givc/modules/pkgs/exec"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_hwidmanager "givc/modules/pkgs/hwidmanager"
	givc_localelistener "givc/modules/pkgs/localelistener"
	givc_notifier "givc/modules/pkgs/notifier"
	givc_policyadmin "givc/modules/pkgs/policyadmin"
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
	applications := config.Capabilities.Applications
	if applications == nil {
		applications = make([]givc_types.ApplicationManifest, 0)
	}

	systemdControlServer, err := givc_servicemanager.NewSystemdControlServer(agentEndpointConfig.Services, applications)
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

	log.Infof("policy-admin: service setup... ")
	// Policy agent server
	if config.Capabilities.Policy.PolicyAdminEnabled {
		log.Infof("policy-admin: service starting... ")
		policyAdminServer, err := givc_policyadmin.NewPolicyAdminServer(config.Capabilities.Policy)
		if err != nil {
			log.Errorf("policy-admin: cannot create policy admin service: %v", err)
		} else {
			log.Infof("policy-admin: service started.")
		}
		grpcServices = append(grpcServices, policyAdminServer)
	}

	// Capability-based services
	if config.Capabilities.Exec.Enabled {
		execServer, err := givc_exec.NewExecServer()
		if err != nil {
			log.Errorf("Cannot create exec server: %v", err)
		} else {
			log.Warnf("Exec capability enabled - allows remote command execution!")
			grpcServices = append(grpcServices, execServer)
		}
	}

	if config.Capabilities.Wifi.Enabled {
		wifiServer, err := givc_wifimanager.NewWifiControlServer()
		if err != nil {
			log.Errorf("Cannot create wifi server: %v", err)
		} else {
			log.Infof("WiFi management capability enabled")
			grpcServices = append(grpcServices, wifiServer)
		}
	}

	if config.Capabilities.Hwid.Enabled {
		hwidServer, err := givc_hwidmanager.NewHwIdServer(config.Capabilities.Hwid.Interface)
		if err != nil {
			log.Errorf("Cannot create hwid server: %v", err)
		} else {
			log.Infof("Hardware ID capability enabled (interface: %s)", config.Capabilities.Hwid.Interface)
			grpcServices = append(grpcServices, hwidServer)
		}
	}

	if config.Capabilities.Notifier.Enabled {
		notifierServer, err := givc_notifier.NewUserNotifierServer(config.Capabilities.Notifier.Socket)
		if err != nil {
			log.Errorf("Cannot create notification server: %v", err)
		} else {
			log.Infof("Notification service capability enabled (socket: %s)", config.Capabilities.Notifier.Socket)
			grpcServices = append(grpcServices, notifierServer)
		}
	}

	if config.Capabilities.Ctap.Enabled {
		ctapServer, err := givc_ctap.NewCtapServer()
		if err != nil {
			log.Errorf("Cannot create ctap server: %v", err)
		} else {
			log.Infof("Ctap service capability enabled")
			grpcServices = append(grpcServices, ctapServer)
		}
	}

	return grpcServices, systemdControlServer, nil
}

// Main function of the GIVC agent.
func main() {
	// Parse command-line arguments
	configFile := flag.String("config", "", "Path to JSON configuration file (required)")
	debugFlag := flag.Bool("debug", false, "Enable debug mode (overrides config file setting)")
	flag.Parse()

	// Validate required arguments
	if *configFile == "" {
		log.Errorf("Configuration file is required. Use -config flag to specify the path.")
		log.Errorf("Usage: %s -config <path-to-config.json> [-debug]", filepath.Base(os.Args[0]))
		os.Exit(1)
	}

	log.Infof("Running %s", filepath.Base(os.Args[0]))
	log.Infof("Configuration file: %s", *configFile)

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

	config, err := givc_config.LoadConfig(*configFile)
	if err != nil {
		log.Errorf("Failed to load configuration: %v", err)
		return
	}

	// Setup log level
	if *debugFlag {
		log.SetLevel(log.DebugLevel)
		log.Debugf("-- Debug mode enabled --")
	} else {
		log.SetLevel(log.WarnLevel)
	}

	log.Debugf("AGENT_CONFIG: %+v", config)
	log.Debugf("AGENT_CONFIG-admin: %+v", config.Network.AdminEndpoint)
	log.Debugf("AGENT_CONFIG-agent: %+v", config.Network.AgentEndpoint)
	log.Debugf("AGENT_CONFIG-tls: %+v", config.Network.TlsConfig)

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
	if config.Capabilities.SocketProxy.Enabled {
		StartSocketService(ctx, &wg, config)
	}
	if config.Capabilities.EventProxy.Enabled {
		StartEventService(ctx, &wg, config)
	}

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
