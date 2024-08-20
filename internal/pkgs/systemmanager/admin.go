// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package systemmanager

import (
	"context"
	"crypto/tls"
	"fmt"
	"givc/internal/pkgs/registry"
	"givc/internal/pkgs/serviceclient"
	"givc/internal/pkgs/types"
	"strings"
	"time"

	"github.com/qmuntal/stateless"
	log "github.com/sirupsen/logrus"
)

const (
	VM_STARTUP_TIME = 10 * time.Second
	WATCH_INTERVAL  = 5 * time.Second
)

type AdminService struct {
	SystemModules []string
	SystemFsm     *stateless.StateMachine
	Registry      *registry.ServiceRegistry
	TlsConfig     *tls.Config
}

func NewAdminService(cfg *types.EndpointConfig) *AdminService {

	svc := &AdminService{
		Registry:  registry.NewRegistry(),
		TlsConfig: cfg.TlsConfig,
	}

	// Initialize required system modules
	for _, service := range cfg.Services {
		if strings.Contains(service, ".service") {
			svc.SystemModules = append(svc.SystemModules, service)
		}
	}

	// Initialize state machine
	svc.SystemFsm = svc.InitSystemStateMachine()
	if svc.SystemFsm == nil {
		log.Fatalln("System state machine could not be initialized")
	}

	svc.SystemFsm.Fire(TIRGGER_INIT_COMPLETE)

	return svc
}

func (svc *AdminService) RegisterService(req *types.RegistryEntry) error {

	// Fetch unit status
	log.Infof("Register attempt: %v", req)

	unitResponse, err := svc.getRemoteStatus(req)
	if err != nil {
		return err
	}
	req.State = *unitResponse

	switch req.Type {
	case types.UNIT_TYPE_HOST_MGR:
		fallthrough
	case types.UNIT_TYPE_SYSVM_MGR:
		fallthrough
	case types.UNIT_TYPE_APPVM_MGR:
		req.Watch = true
	}

	// Create new registry entry
	err = svc.Registry.Register(req)
	if err != nil {
		log.Errorf("Error registering entry: %s", err)
	}

	// Check state and transition
	registerHostPhase, err := svc.SystemFsm.IsInState(STATE_REGISTER_HOST)
	if err != nil {
		return fmt.Errorf("error fetching system state")
	}
	if registerHostPhase {
		if req.Type == types.UNIT_TYPE_HOST_MGR { // verify?
			svc.SystemFsm.Fire(TRIGGER_HOST_REGISTERED)
		}
	}

	registerVmsPhase, err := svc.SystemFsm.IsInState(STATE_REGISTER_VMS)
	if err != nil {
		return fmt.Errorf("error fetching system state")
	}
	if registerVmsPhase {
		for _, service := range svc.SystemModules {
			serviceEntry := svc.Registry.GetEntryByName(service)
			if serviceEntry == nil {
				log.Infof("%s not yet registered", service)
				return nil
			}
		}
		svc.SystemFsm.Fire(TRIGGER_VMS_REGISTERED)
	}

	state, _ := svc.SystemFsm.State(context.Background())
	log.Infof("STATE: %s", state)

	return nil
}

/*
Previously assumed that 'name' is started in microvm@'name'-vm.service
Quickfix allows for 'application:vm-name' to be passed as 'name'.
@TODO: Properly address this in rust admin service implementation
*/
func (svc *AdminService) StartApplication(name string) (string, error) {

	cmdFailure := "Command failed."

	isRunning, err := svc.SystemFsm.IsInState(STATE_RUN)
	if err != nil {
		return cmdFailure, fmt.Errorf("error determining system state")
	}
	if !isRunning {
		// return cmdFailure, fmt.Errorf("cannot start application, not all required system-vms are registered")
		log.Warnf("not all required system-vms are registered")
	}

	// Check registry for applications' systemd agent
	vmName := name + "-vm"
	if strings.Contains(name, ":") {
		appString := strings.Split(name, ":")
		name = appString[0]
		vmName = appString[1]
	}
	systemdAgent := "givc-" + vmName + ".service"
	regEntry := svc.Registry.GetEntryByName(systemdAgent)

	// If agent is not found, start VM
	if regEntry == nil {
		err = svc.startVM(name)
		if err != nil {
			return cmdFailure, fmt.Errorf("cannot start application vm")
		}
		regEntry = svc.Registry.GetEntryByName(systemdAgent)
		if regEntry == nil {
			return cmdFailure, fmt.Errorf("cannot start application, app-vm not found")
		}
	}

	// Configure service manager endpoint
	clientCfg := &types.EndpointConfig{
		Transport: regEntry.Transport,
		TlsConfig: svc.TlsConfig,
	}

	// Get unique name from registry
	serviceName := svc.Registry.GetUniqueEntryName(name)

	// Start application
	_, err = serviceclient.StartRemoteApplication(clientCfg, serviceName)
	if err != nil {
		return cmdFailure, err
	}

	// Check execution status
	statusResponse, err := serviceclient.GetRemoteStatus(clientCfg, serviceName)
	if err != nil {
		return cmdFailure, fmt.Errorf("cannot retrieve unit status for %s: %v", name, err)
	}
	if statusResponse.ActiveState != "active" {
		return cmdFailure, fmt.Errorf("cannot start unit %s", serviceName)
	}

	// Register application
	appEntry := &types.RegistryEntry{
		Name:   serviceName,
		Parent: systemdAgent,
		Type:   types.UNIT_TYPE_APPVM_APP,
		State:  *statusResponse,
		Watch:  true,
	}
	err = svc.Registry.Register(appEntry)
	if err != nil {
		return cmdFailure, fmt.Errorf("failed to register %s: %v", serviceName, err)
	}

	return "Command successful.", nil
}

func (svc *AdminService) PauseApplication(name string) (string, error) {

	cmdFailure := "Command failed."

	isRunning, err := svc.SystemFsm.IsInState(STATE_RUN)
	if err != nil {
		return cmdFailure, fmt.Errorf("error determining system state")
	}
	if !isRunning {
		return cmdFailure, fmt.Errorf("cannot pause application, not all required system-vms are registered")
	}

	// Check registry for applications' systemd agent
	vmName := name
	if strings.Contains(name, ":") {
		appString := strings.Split(name, ":")
		name = appString[0]
		vmName = appString[1]
	}
	systemdAgent := "givc-" + vmName + ".service"
	regEntry := svc.Registry.GetEntryByName(systemdAgent)

	// If agent is not found, return error
	if regEntry == nil {
		return cmdFailure, fmt.Errorf("cannot pause application, agent not found")
	}

	// Configure service manager endpoint
	clientCfg := &types.EndpointConfig{
		Transport: regEntry.Transport,
		TlsConfig: svc.TlsConfig,
	}

	// Determine if multiple services to pause
	if !strings.Contains(name, "@") {
		entries := svc.Registry.GetEntriesByName(name)
		for _, entry := range entries {
			// Pause application
			_, err = serviceclient.PauseRemoteService(clientCfg, entry.Name)
			if err != nil {
				return cmdFailure, err
			}
		}
	} else {
		// Pause application
		_, err = serviceclient.PauseRemoteService(clientCfg, name)
		if err != nil {
			return cmdFailure, err
		}
	}

	return "Command successful.", nil
}

func (svc *AdminService) ResumeApplication(name string) (string, error) {

	cmdFailure := "Command failed."

	isRunning, err := svc.SystemFsm.IsInState(STATE_RUN)
	if err != nil {
		return cmdFailure, fmt.Errorf("error determining system state")
	}
	if !isRunning {
		return cmdFailure, fmt.Errorf("cannot resume application, not all required system-vms are registered")
	}

	// Check registry for applications' systemd agent
	vmName := name
	if strings.Contains(name, ":") {
		appString := strings.Split(name, ":")
		name = appString[0]
		vmName = appString[1]
	}
	systemdAgent := "givc-" + vmName + ".service"
	regEntry := svc.Registry.GetEntryByName(systemdAgent)

	// If agent is not found, return error
	if regEntry == nil {
		return cmdFailure, fmt.Errorf("cannot resume application, agent not found")
	}

	// Configure service manager endpoint
	clientCfg := &types.EndpointConfig{
		Transport: regEntry.Transport,
		TlsConfig: svc.TlsConfig,
	}

	// Determine if multiple services to resume
	if !strings.Contains(name, "@") {
		entries := svc.Registry.GetEntriesByName(name)
		for _, entry := range entries {
			// Resume application
			_, err = serviceclient.ResumeRemoteService(clientCfg, entry.Name)
			if err != nil {
				return cmdFailure, err
			}
		}
	} else {
		// Resume application
		_, err = serviceclient.ResumeRemoteService(clientCfg, name)
		if err != nil {
			return cmdFailure, err
		}
	}

	return "Command successful.", nil
}

func (svc *AdminService) StopApplication(name string) (string, error) {

	cmdFailure := "Command failed."

	isRunning, err := svc.SystemFsm.IsInState(STATE_RUN)
	if err != nil {
		return cmdFailure, fmt.Errorf("error determining system state")
	}
	if !isRunning {
		return cmdFailure, fmt.Errorf("cannot stop application, not all required system-vms are registered")
	}

	// Check registry for applications' systemd agent
	vmName := name
	if strings.Contains(name, ":") {
		appString := strings.Split(name, ":")
		name = appString[0]
		vmName = appString[1]
	}
	systemdAgent := "givc-" + vmName + ".service"
	regEntry := svc.Registry.GetEntryByName(systemdAgent)

	// If agent is not found, return error
	if regEntry == nil {
		return cmdFailure, fmt.Errorf("cannot stop application, agent not found")
	}

	// Configure service manager endpoint
	clientCfg := &types.EndpointConfig{
		Transport: regEntry.Transport,
		TlsConfig: svc.TlsConfig,
	}

	// Determine if multiple services to stop
	if !strings.Contains(name, "@") {
		entries := svc.Registry.GetEntriesByName(name)
		for _, entry := range entries {
			// Stop application
			_, err = serviceclient.StopRemoteService(clientCfg, entry.Name)
			if err != nil {
				return cmdFailure, err
			}
		}
	} else {
		// Stop application
		_, err = serviceclient.StopRemoteService(clientCfg, name)
		if err != nil {
			return cmdFailure, err
		}
	}

	return "Command successful.", nil
}

func (svc *AdminService) Poweroff() error {
	return svc.sendSystemCommand("poweroff.target")
}

func (svc *AdminService) Reboot() error {
	return svc.sendSystemCommand("reboot.target")
}

func (svc *AdminService) Monitor() {

	for {
		// Get current list of monitored units
		watchlist := svc.Registry.GetWatchList()
		for _, entry := range watchlist {

			// Query remote status
			statusResponse, err := svc.getRemoteStatus(entry)
			if err != nil {
				log.Warnf("could not get status of unit %s", entry.Name)
				// @TODO Add error handling if unit is un-responsive
				svc.handleError(entry)
				break
			}
			if statusResponse.ActiveState != "active" {
				// @TODO Add handling if unit is not active
				log.Warnf("unit %s is inactive", entry.Name)
				svc.handleError(entry)
				break
			}

			// Update state
			entry.State = *statusResponse
		}
		time.Sleep(WATCH_INTERVAL)
	}
}
