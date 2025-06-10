// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Package serviceclient provides functionality to interact with remote services via gRPC.
package serviceclient

import (
	"context"
	givc_admin "givc/modules/api/admin"
	givc_systemd "givc/modules/api/systemd"
	givc_grpc "givc/modules/pkgs/grpc"
	"givc/modules/pkgs/types"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

// GetRemoteStatus retrieves the status of a remote service by its unit name.
func GetRemoteStatus(cfg *types.EndpointConfig, unitName string) (*types.UnitStatus, error) {

	// Setup and dial GRPC client
	var conn *grpc.ClientConn
	conn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		log.Errorf("Cannot create grpc client: %v", err)
		return nil, err
	}
	defer conn.Close()

	// Create client
	client := givc_systemd.NewUnitControlServiceClient(conn)
	if client == nil {
		log.Errorf("Failed to create 'NewUnitControlServiceClient'")
		return nil, err
	}

	// Get unit status
	request := givc_systemd.UnitRequest{
		UnitName: unitName,
	}
	ctx := context.Background()
	resp, err := client.GetUnitStatus(ctx, &request)
	if err != nil {
		log.Errorf("Not the response we hoped for: %v", err)
		return nil, err
	}

	response := &types.UnitStatus{
		Name:         resp.UnitStatus.Name,
		Description:  resp.UnitStatus.Description,
		LoadState:    resp.UnitStatus.LoadState,
		ActiveState:  resp.UnitStatus.ActiveState,
		SubState:     resp.UnitStatus.SubState,
		Path:         string(resp.UnitStatus.Path),
		FreezerState: resp.UnitStatus.FreezerState,
	}

	return response, nil
}

// RegisterRemoteService registers a givc agent and its services with the admin server.
func RegisterRemoteService(cfg *types.EndpointConfig, reg *givc_admin.RegistryRequest) (*givc_admin.RegistryResponse, error) {

	// Setup and dial GRPC client
	var conn *grpc.ClientConn
	conn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		log.Errorf("Cannot create grpc client: %v", err)
		return nil, err
	}
	defer conn.Close()

	// Create admin client
	client := givc_admin.NewAdminServiceClient(conn)
	if client == nil {
		log.Errorf("Failed to create new admin client")
		return nil, err
	}

	// Send registry request
	log.Infof("Sending request: %v", reg)
	ctx := context.Background()
	resp, err := client.RegisterService(ctx, reg)
	if err != nil {
		log.Errorf("Not the response we hoped for: %v", err)
		return nil, err
	}
	log.Infoln(resp)

	return resp, nil
}

// StartRemoteService starts a remote service at an endpoint by its unit name.
func StartRemoteService(cfg *types.EndpointConfig, unitName string) (*givc_systemd.UnitResponse, error) {

	// Setup and dial GRPC client
	var conn *grpc.ClientConn
	conn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		log.Errorf("Cannot create grpc client: %v", err)
		return nil, err
	}
	defer conn.Close()

	// Create client
	client := givc_systemd.NewUnitControlServiceClient(conn)
	if client == nil {
		log.Errorf("Failed to create 'NewUnitControlServiceClient'")
		return nil, err
	}

	// Start unit
	request := givc_systemd.UnitRequest{
		UnitName: unitName,
	}
	ctx := context.Background()
	resp, err := client.StartUnit(ctx, &request)
	if err != nil {
		log.Errorf("Not the response we hoped for: %v", err)
		return nil, err
	}

	log.Infoln(resp)
	return resp, nil
}

// PauseRemoteService pauses a remote service at an endpoint by its unit name.
func PauseRemoteService(cfg *types.EndpointConfig, unitName string) (*givc_systemd.UnitResponse, error) {

	// Setup and dial GRPC client
	var conn *grpc.ClientConn
	conn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		log.Errorf("Cannot create grpc client: %v", err)
		return nil, err
	}
	defer conn.Close()

	// Create client
	client := givc_systemd.NewUnitControlServiceClient(conn)
	if client == nil {
		log.Errorf("Failed to create 'NewUnitControlServiceClient'")
		return nil, err
	}

	// Start unit
	request := givc_systemd.UnitRequest{
		UnitName: unitName,
	}
	ctx := context.Background()
	resp, err := client.FreezeUnit(ctx, &request)
	if err != nil {
		log.Errorf("Not the response we hoped for: %v", err)
		return nil, err
	}

	log.Infoln(resp)
	return resp, nil
}

// ResumeRemoteService resumes a remote service at an endpoint by its unit name.
func ResumeRemoteService(cfg *types.EndpointConfig, unitName string) (*givc_systemd.UnitResponse, error) {

	// Setup and dial GRPC client
	var conn *grpc.ClientConn
	conn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		log.Errorf("Cannot create grpc client: %v", err)
		return nil, err
	}
	defer conn.Close()

	// Create client
	client := givc_systemd.NewUnitControlServiceClient(conn)
	if client == nil {
		log.Errorf("Failed to create 'NewUnitControlServiceClient'")
		return nil, err
	}

	// Start unit
	request := givc_systemd.UnitRequest{
		UnitName: unitName,
	}
	ctx := context.Background()
	resp, err := client.UnfreezeUnit(ctx, &request)
	if err != nil {
		log.Errorf("Not the response we hoped for: %v", err)
		return nil, err
	}

	log.Infoln(resp)
	return resp, nil
}

// StopRemoteService stops a remote service at an endpoint by its unit name.
func StopRemoteService(cfg *types.EndpointConfig, unitName string) (*givc_systemd.UnitResponse, error) {

	// Setup and dial GRPC client
	var conn *grpc.ClientConn
	conn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		log.Errorf("Cannot create grpc client: %v", err)
		return nil, err
	}
	defer conn.Close()

	// Create client
	client := givc_systemd.NewUnitControlServiceClient(conn)
	if client == nil {
		log.Errorf("Failed to create 'NewUnitControlServiceClient'")
		return nil, err
	}

	// Start unit
	request := givc_systemd.UnitRequest{
		UnitName: unitName,
	}
	ctx := context.Background()
	resp, err := client.StopUnit(ctx, &request)
	if err != nil {
		log.Errorf("Not the response we hoped for: %v", err)
		return nil, err
	}

	log.Infoln(resp)
	return resp, nil
}
