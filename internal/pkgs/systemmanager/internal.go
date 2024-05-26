// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package systemmanager

import (
	"fmt"
	"givc/internal/pkgs/serviceclient"
	"givc/internal/pkgs/types"
	"strings"
	"time"

	log "github.com/sirupsen/logrus"
)

func (svc *AdminService) getHostConfig() (*types.EndpointConfig, error) {

	// Get host entry
	entries := svc.Registry.GetEntryByType(types.UNIT_TYPE_HOST_MGR)
	if len(entries) < 1 {
		return nil, fmt.Errorf("required host manager not registered")
	}
	if len(entries) > 1 {
		return nil, fmt.Errorf("more than one host manager registered: %v", entries) //oO
	}
	host := entries[0]

	// Configure host endpoint
	hostCfg := &types.EndpointConfig{
		Transport: host.Transport,
		TlsConfig: svc.TlsConfig,
	}
	return hostCfg, nil
}

func (svc *AdminService) getRemoteStatus(req *types.RegistryEntry) (*types.UnitStatus, error) {

	// Configure client endpoint
	clientTransportParams := req.Transport
	if clientTransportParams.Address == "" {
		parent := svc.Registry.GetEntryByName(req.Parent)
		if parent == nil {
			return nil, fmt.Errorf("entry has no parent or address")
		}
		clientTransportParams = parent.Transport
	}

	cfgClient := &types.EndpointConfig{
		Transport: clientTransportParams,
		TlsConfig: svc.TlsConfig,
	}

	// Fetch status info
	resp, err := serviceclient.GetRemoteStatus(cfgClient, req.Name)
	if err != nil {
		log.Errorf("cannot retrieve unit status for %s: %v\n", req.Name, err)
		return nil, err
	}
	return resp, nil
}

func (svc *AdminService) startVM(name string) error {

	// Get host config
	hostCfg, err := svc.getHostConfig()
	if err != nil {
		return fmt.Errorf("cannot retrieve host entry %s", err)
	}

	// Check status and start service
	vmName := "microvm@" + name + "-vm.service"
	statusResponse, err := serviceclient.GetRemoteStatus(hostCfg, vmName)
	if err != nil {
		return fmt.Errorf("cannot retrieve vm status %s: %v", vmName, err)
	}
	if statusResponse.LoadState != "loaded" {
		return fmt.Errorf("vm %s not loaded", vmName)
	}
	if statusResponse.ActiveState != "active" {
		_, err := serviceclient.StartRemoteService(hostCfg, vmName)
		if err != nil {
			return err
		}
		time.Sleep(VM_STARTUP_TIME)
		statusResponse, err := serviceclient.GetRemoteStatus(hostCfg, vmName)
		if err != nil {
			return fmt.Errorf("cannot retrieve vm status for %s: %v", vmName, err)
		}
		if statusResponse.ActiveState != "active" {
			// @TODO this may throw an error currently if unit not yet started
			return fmt.Errorf("cannot start vm %s", vmName)
		}
	}
	return nil
}

func (svc *AdminService) sendSystemCommand(name string) error {

	// Get host config
	hostCfg, err := svc.getHostConfig()
	if err != nil {
		return fmt.Errorf("cannot retrieve host entry %s", err)
	}

	// Start unit
	_, err = serviceclient.StartRemoteService(hostCfg, name)
	if err != nil {
		return err
	}
	return nil
}

func (svc *AdminService) handleError(entry *types.RegistryEntry) {

	var err error
	switch entry.Type {
	case types.UNIT_TYPE_APPVM_APP:
		// Application handling
		err = svc.Registry.Deregister(entry)
		if err != nil {
			log.Warnf("cannot de-register service: %s", err)
		}
	case types.UNIT_TYPE_APPVM_MGR:
		fallthrough
	case types.UNIT_TYPE_SYSVM_MGR:
		// If agent is not found, re-start VM
		name := strings.Split(entry.Name, "-vm.service")[0]
		name = strings.Split(name, "givc-")[1]
		err = svc.startVM(name)
		if err != nil {
			log.Errorf("cannot start vm for %s", name)
		}
	}
}
