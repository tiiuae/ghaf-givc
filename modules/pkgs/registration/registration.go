// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Service registration functionality for the GIVC agent.
package registration

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"time"

	givc_admin "givc/modules/api/admin"
	givc_systemd "givc/modules/api/systemd"
	givc_config "givc/modules/pkgs/config"
	givc_serviceclient "givc/modules/pkgs/serviceclient"
	givc_servicemanager "givc/modules/pkgs/servicemanager"

	log "github.com/sirupsen/logrus"
)

// RegistrationConfig holds the configuration needed for service registration
type RegistrationConfig struct {
	SystemdServer *givc_servicemanager.SystemdControlServer
	AgentConfig   *givc_config.AgentConfig
}

// Registry defines the interface for service registration operations
type Registry interface {
	// RegisterAgent registers the agent with the admin server
	RegisterAgent(ctx context.Context) error

	// RegisterServices registers all configured services with the admin server
	RegisterServices(ctx context.Context) error

	// StartRegistrationWorker starts the registration process
	StartRegistrationWorker(ctx context.Context, wg *sync.WaitGroup, serverStarted <-chan struct{})
}

// ServiceRegistry implements the Registry interface
type ServiceRegistry struct {
	config RegistrationConfig
}

// NewServiceRegistry creates a new ServiceRegistry with the given configuration
func NewServiceRegistry(config RegistrationConfig) Registry {
	return &ServiceRegistry{
		config: config,
	}
}

// StartRegistrationWorker starts the registration process
func (r *ServiceRegistry) StartRegistrationWorker(ctx context.Context, wg *sync.WaitGroup, serverStarted <-chan struct{}) {
	wg.Add(1)
	go func() {
		defer wg.Done()

		// Wait for server to start to handle callbacks
		select {
		case <-serverStarted:
		case <-ctx.Done():
			log.Infof("Registration cancelled before server start")
			return
		}

		// Register agent
		if err := r.RegisterAgent(ctx); err != nil {
			log.Errorf("Failed to register agent: %v", err)
			return
		}

		// Register services (systemd units)
		if err := r.RegisterServices(ctx); err != nil {
			log.Errorf("Failed to register services: %v", err)
			return
		}
		log.Infof("Registration goroutine finished")
	}()
}

// RegisterAgent registers the agent with the admin server
func (r *ServiceRegistry) RegisterAgent(ctx context.Context) error {
	if r.config.SystemdServer == nil {
		return fmt.Errorf("systemd server not configured")
	}

	agentServiceName := r.config.AgentConfig.Identity.ServiceName
	if agentServiceName == "" {
		return fmt.Errorf("agent service name not configured")
	}

	unitStatus, err := r.config.SystemdServer.GetUnitStatus(ctx, &givc_systemd.UnitRequest{
		UnitName: agentServiceName,
	})
	if err != nil {
		return err
	}

	agentEntryRequest := &givc_admin.RegistryRequest{
		Name:   agentServiceName,
		Type:   r.config.AgentConfig.Identity.Type,
		Parent: r.config.AgentConfig.Identity.Parent,
		Transport: &givc_admin.TransportConfig{
			Protocol: r.config.AgentConfig.Network.AgentEndpoint.Transport.Protocol,
			Address:  r.config.AgentConfig.Network.AgentEndpoint.Transport.Address,
			Port:     r.config.AgentConfig.Network.AgentEndpoint.Transport.Port,
			Name:     r.config.AgentConfig.Network.AgentEndpoint.Transport.Name,
		},
		State: unitStatus.UnitStatus,
	}

	// Register agent with admin server with retry loop
	return r.registerWithRetry(ctx, agentEntryRequest, "agent")
}

// RegisterServices registers all configured services with the admin server
func (r *ServiceRegistry) RegisterServices(ctx context.Context) error {
	for service, subType := range r.config.AgentConfig.Capabilities.Units {
		if !strings.HasSuffix(service, ".service") {
			continue
		}

		select {
		case <-ctx.Done():
			log.Infof("Service registration cancelled")
			return ctx.Err()
		default:
		}

		if err := r.registerSingleService(ctx, service, subType); err != nil {
			log.Warnf("Failed to register service %s: %v", service, err)
			// Continue with other services even if one fails
			continue
		}
	}

	return nil
}

// registerSingleService registers a single service with the admin server
func (r *ServiceRegistry) registerSingleService(ctx context.Context, service string, subType uint32) error {
	if r.config.SystemdServer == nil {
		return fmt.Errorf("cannot register service %s: systemd server not configured", service)
	}

	unitStatus, err := r.config.SystemdServer.GetUnitStatus(ctx, &givc_systemd.UnitRequest{
		UnitName: service,
	})
	if err != nil {
		log.Warnf("Error getting unit status for %s: %s", service, err)
		return err
	}

	serviceEntryRequest := &givc_admin.RegistryRequest{
		Name:   service,
		Parent: r.config.AgentConfig.Identity.ServiceName,
		Type:   uint32(subType),
		Transport: &givc_admin.TransportConfig{
			Name:     r.config.AgentConfig.Network.AgentEndpoint.Transport.Name,
			Protocol: r.config.AgentConfig.Network.AgentEndpoint.Transport.Protocol,
			Address:  r.config.AgentConfig.Network.AgentEndpoint.Transport.Address,
			Port:     r.config.AgentConfig.Network.AgentEndpoint.Transport.Port,
		},
		State: unitStatus.UnitStatus,
	}

	log.Infof("Trying to register service: %s", service)
	_, err = givc_serviceclient.RegisterRemoteService(r.config.AgentConfig.Network.AdminEndpoint, serviceEntryRequest)
	if err != nil {
		log.Warnf("Error registering service %s: %s", service, err)
		return err
	} else {
		log.Infof("Successfully registered service: %s", service)
	}

	return nil
}

// registerWithRetry performs registration with retry logic
func (r *ServiceRegistry) registerWithRetry(ctx context.Context, request *givc_admin.RegistryRequest, entityType string) error {
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
			_, err := givc_serviceclient.RegisterRemoteService(r.config.AgentConfig.Network.AdminEndpoint, request)
			if err == nil {
				log.Infof("Successfully registered %s: %s", entityType, request.Name)
				return nil
			}
			log.Warnf("Error registering %s: %s, retrying...", entityType, err)
			time.Sleep(1 * time.Second)
		}
	}
}
