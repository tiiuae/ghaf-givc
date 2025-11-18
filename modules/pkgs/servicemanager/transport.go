// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// The servicemanager package provides functionality to manage systemd services and applications.
package servicemanager

import (
	"context"
	"fmt"
	"time"

	givc_systemd "givc/modules/api/systemd"
	"givc/modules/pkgs/types"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
	grpc_codes "google.golang.org/grpc/codes"
	grpc_status "google.golang.org/grpc/status"
)

const (
	ResourceStreamInterval = 400 * time.Millisecond
)

type SystemdControlServer struct {
	Controller *SystemdController
	givc_systemd.UnimplementedUnitControlServiceServer
}

func (s *SystemdControlServer) Name() string {
	return "Systemd Control Server"
}

func (s *SystemdControlServer) RegisterGrpcService(srv *grpc.Server) {
	givc_systemd.RegisterUnitControlServiceServer(srv, s)
}

// NewSystemdControlServer creates a new instance of SystemdControlServer with the provided service whitelist and applications.
func NewSystemdControlServer(whitelist []string, applications []types.ApplicationManifest) (*SystemdControlServer, error) {

	systemdController, err := NewController(whitelist, applications)
	if err != nil {
		log.Errorf("Error creating systemd controller: %v", err)
		return nil, err
	}

	systemdControlServer := SystemdControlServer{
		Controller: systemdController,
	}

	return &systemdControlServer, nil
}

func (s *SystemdControlServer) Close() {
	s.Controller.Close()
}

// getUnitStatus fetches the status of a systemd unit by its name.
func (s *SystemdControlServer) getUnitStatus(ctx context.Context, name string) (*givc_systemd.UnitStatus, error) {

	unitStatus, err := s.Controller.FindUnit(name)
	if err != nil {
		log.Infof("[GetUnitStatus] Error finding unit: %v", err)
		return nil, grpc_status.Error(grpc_codes.NotFound, fmt.Sprintf("error fetching unit status: %s", name))
	}
	if len(unitStatus) != 1 {
		errStr := fmt.Sprintf("error, got %d units named %s", len(unitStatus), name)
		return nil, grpc_status.Error(grpc_codes.NotFound, errStr)
	}

	freezerState, err := s.Controller.getUnitPropertyString(ctx, name, "FreezerState")
	if err != nil {
		log.Infof("[GetUnitStatus] Error fetching freezer state: %v\n", err)
		freezerState = "error"
	}

	resp := &givc_systemd.UnitStatus{
		Name:         unitStatus[0].Name,
		Description:  unitStatus[0].Description,
		LoadState:    unitStatus[0].LoadState,
		ActiveState:  unitStatus[0].ActiveState,
		SubState:     unitStatus[0].SubState,
		Path:         string(unitStatus[0].Path),
		FreezerState: freezerState,
	}

	return resp, nil
}

// GetUnitStatus handles the gRPC request to get the status of a systemd unit.
func (s *SystemdControlServer) GetUnitStatus(ctx context.Context, req *givc_systemd.UnitRequest) (*givc_systemd.UnitResponse, error) {
	log.Infof("Incoming request to fetch unit status: %v\n", req)
	unitStatus, err := s.getUnitStatus(ctx, req.UnitName)
	if err != nil {
		log.Infof("[GetUnitStatus] Error getting unit status: %v", err)
		return nil, err
	}
	resp := &givc_systemd.UnitResponse{
		UnitStatus: unitStatus,
	}
	return resp, nil
}

// StartUnit handles the gRPC request to start a systemd unit.
func (s *SystemdControlServer) StartUnit(ctx context.Context, req *givc_systemd.UnitRequest) (*givc_systemd.UnitResponse, error) {
	log.Infof("Incoming request to (re)start %v\n", req)

	err := s.Controller.StartUnit(ctx, req.UnitName)
	if err != nil {
		log.Infof("[StartUnit] Error starting unit: %v", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("cannot start unit: %s", err))
	}
	unitStatus, err := s.getUnitStatus(ctx, req.UnitName)
	if err != nil {
		return nil, err
	}
	resp := &givc_systemd.UnitResponse{
		UnitStatus: unitStatus,
	}
	return resp, nil
}

// StopUnit handles the gRPC request to stop a systemd unit.
func (s *SystemdControlServer) StopUnit(ctx context.Context, req *givc_systemd.UnitRequest) (*givc_systemd.UnitResponse, error) {
	log.Infof("Incoming request to stop %v\n", req)

	err := s.Controller.StopUnit(ctx, req.UnitName)
	if err != nil {
		log.Infof("[StopUnit] Error stopping unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("cannot stop unit: %s", err))
	}
	unitStatus, err := s.getUnitStatus(ctx, req.UnitName)
	if err != nil {
		// Override for transient units
		unitStatus = &givc_systemd.UnitStatus{
			Name:        req.UnitName,
			ActiveState: "inactive",
			SubState:    "dead",
		}
	}
	resp := &givc_systemd.UnitResponse{
		UnitStatus: unitStatus,
	}
	return resp, nil
}

// KillUnit handles the gRPC request to kill a systemd unit.
func (s *SystemdControlServer) KillUnit(ctx context.Context, req *givc_systemd.UnitRequest) (*givc_systemd.UnitResponse, error) {
	log.Infof("Incoming request to kill %v\n", req)

	err := s.Controller.KillUnit(ctx, req.UnitName)
	if err != nil {
		log.Infof("[KillUnit] Error killing unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("cannot kill unit: %s", err))
	}
	unitStatus, err := s.getUnitStatus(ctx, req.UnitName)
	if err != nil {
		// Override for transient units
		unitStatus = &givc_systemd.UnitStatus{
			Name:        req.UnitName,
			ActiveState: "inactive",
			SubState:    "dead",
		}
	}
	resp := &givc_systemd.UnitResponse{
		UnitStatus: unitStatus,
	}
	return resp, nil
}

// FreezeUnit handles the gRPC request to freeze (pause) a systemd unit.
func (s *SystemdControlServer) FreezeUnit(ctx context.Context, req *givc_systemd.UnitRequest) (*givc_systemd.UnitResponse, error) {
	log.Infof("Incoming request to freeze %v", req)

	err := s.Controller.FreezeUnit(ctx, req.UnitName)
	if err != nil {
		log.Infof("[FreezeUnit] Error freezing unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("cannot freeze unit: %s", err))
	}
	unitStatus, err := s.getUnitStatus(ctx, req.UnitName)
	if err != nil {
		return nil, err
	}
	resp := &givc_systemd.UnitResponse{
		UnitStatus: unitStatus,
	}
	return resp, nil
}

// UnfreezeUnit handles the gRPC request to unfreeze (unpause) a systemd unit.
func (s *SystemdControlServer) UnfreezeUnit(ctx context.Context, req *givc_systemd.UnitRequest) (*givc_systemd.UnitResponse, error) {
	log.Infof("Incoming request to unfreeze %v\n", req)

	err := s.Controller.UnfreezeUnit(ctx, req.UnitName)
	if err != nil {
		log.Infof("[UnfreezeUnit] Error unfreezing unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("cannot unfreeze unit: %s", err))
	}
	unitStatus, err := s.getUnitStatus(ctx, req.UnitName)
	if err != nil {
		return nil, err
	}
	resp := &givc_systemd.UnitResponse{
		UnitStatus: unitStatus,
	}
	return resp, nil
}

// MonitorUnit handles the gRPC request to monitor a systemd unit's resource usage.
// This is legacy code and will be removed.
func (s *SystemdControlServer) MonitorUnit(req *givc_systemd.UnitResourceRequest, stream givc_systemd.UnitControlService_MonitorUnitServer) error {
	log.Infof("Incoming resource monitor request for %v\n", req)

	// Find unit
	units, err := s.Controller.FindUnit(req.UnitName)
	if err != nil {
		return grpc_status.Error(grpc_codes.NotFound, fmt.Sprintf("cannot monitor unit: %s", err))
	}
	if len(units) != 1 {
		return fmt.Errorf("none or more than one unit found")
	}
	unit := units[0]

	if unit.ActiveState != "active" {
		return fmt.Errorf("unit %s is %s", unit.Name, unit.ActiveState)
	}

	// Get pid from unit property or pid
	unitProps, err := s.Controller.getUnitProperties(context.Background(), unit.Name)
	if err != nil {
		return err
	}

	pid, ok := unitProps["MainPID"].(uint32)
	if !ok || pid == 0 {
		return fmt.Errorf("failed to unwrap integer value from dbus.Variant")
	}

	for i := 0; i < 50; i += 1 {
		cpuUsage, memoryUsage, err := s.Controller.getUnitCpuAndMem(context.Background(), pid)
		if err != nil {
			log.Infof("[MonitorUnit] Error fetching unit properties: %v\n", err)
			return fmt.Errorf("cannot fetch unit properties")
		}
		resp := &givc_systemd.UnitResourceResponse{
			CpuUsage:    cpuUsage,
			MemoryUsage: memoryUsage,
		}
		if err := stream.Send(resp); err != nil {
			return err
		}
		time.Sleep(ResourceStreamInterval)
	}
	return nil
}

// StartApplication handles the gRPC request to start an application unit.
func (s *SystemdControlServer) StartApplication(ctx context.Context, req *givc_systemd.AppUnitRequest) (*givc_systemd.UnitResponse, error) {
	log.Infof("Executing application start method for: %s\n", req.UnitName)

	unitStatus, err := s.Controller.StartApplication(ctx, req.UnitName, req.Args)
	if err != nil {
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("cannot start application: %s", err))
	}

	freezerState, err := s.Controller.getUnitPropertyString(ctx, unitStatus.Name, "FreezerState")
	if err != nil {
		log.Infof("[StartApplication] Error fetching freezer state: %v\n", err)
		freezerState = "error"
	}

	resp := &givc_systemd.UnitResponse{
		UnitStatus: &givc_systemd.UnitStatus{
			Name:         unitStatus.Name,
			Description:  unitStatus.Description,
			LoadState:    unitStatus.LoadState,
			ActiveState:  unitStatus.ActiveState,
			SubState:     unitStatus.SubState,
			Path:         string(unitStatus.Path),
			FreezerState: freezerState,
		},
	}

	return resp, nil
}
