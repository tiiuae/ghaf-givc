// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package servicemanager

import (
	"context"
	"fmt"

	"time"

	systemd_api "givc/api/systemd"
	"givc/internal/pkgs/types"

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
	systemd_api.UnimplementedUnitControlServiceServer
}

func (s *SystemdControlServer) Name() string {
	return "Systemd Control Server"
}

func (s *SystemdControlServer) RegisterGrpcService(srv *grpc.Server) {
	systemd_api.RegisterUnitControlServiceServer(srv, s)
}

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

func (s *SystemdControlServer) GetUnitStatus(ctx context.Context, req *systemd_api.UnitRequest) (*systemd_api.UnitStatusResponse, error) {
	log.Infof("Incoming request to fetch unit status: %v\n", req)

	unitStatus, err := s.Controller.FindUnit(req.UnitName)
	if err != nil {
		log.Infof("[GetUnitStatus] Error finding unit: %v", err)
		return nil, grpc_status.Error(grpc_codes.NotFound, "error fetching unit status")
	}
	if len(unitStatus) != 1 {
		errStr := fmt.Sprintf("error, got %d units named %s", len(unitStatus), req.UnitName)
		return nil, grpc_status.Error(grpc_codes.NotFound, errStr)
	}

	freezerState, err := s.Controller.GetUnitPropertyString(context.Background(), req.UnitName, "FreezerState")
	if err != nil {
		log.Infof("[GetUnitStatus] Error fetching freezer state: %v\n", err)
		freezerState = "error"
	}

	resp := &systemd_api.UnitStatusResponse{
		CmdStatus: "Command successful",
		UnitStatus: &systemd_api.UnitStatus{
			Name:         unitStatus[0].Name,
			Description:  unitStatus[0].Description,
			LoadState:    unitStatus[0].LoadState,
			ActiveState:  unitStatus[0].ActiveState,
			SubState:     unitStatus[0].SubState,
			Path:         string(unitStatus[0].Path),
			FreezerState: freezerState,
		},
	}

	return resp, nil
}

func (s *SystemdControlServer) StartUnit(ctx context.Context, req *systemd_api.UnitRequest) (*systemd_api.UnitResponse, error) {
	log.Infof("Incoming request to (re)start %v\n", req)

	err := s.Controller.StartUnit(context.Background(), req.UnitName)
	if err != nil {
		log.Infof("[StartUnit] Error starting unit: %v", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, "cannot start unit")
	}
	return &systemd_api.UnitResponse{CmdStatus: "Command successful"}, nil
}

func (s *SystemdControlServer) StopUnit(ctx context.Context, req *systemd_api.UnitRequest) (*systemd_api.UnitResponse, error) {
	log.Infof("Incoming request to stop %v\n", req)

	err := s.Controller.StopUnit(context.Background(), req.UnitName)
	if err != nil {
		log.Infof("[StopUnit] Error stopping unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, "cannot stop unit")
	}
	return &systemd_api.UnitResponse{CmdStatus: "Command successful"}, nil
}

func (s *SystemdControlServer) KillUnit(ctx context.Context, req *systemd_api.UnitRequest) (*systemd_api.UnitResponse, error) {
	log.Infof("Incoming request to kill %v\n", req)

	err := s.Controller.KillUnit(context.Background(), req.UnitName)
	if err != nil {
		log.Infof("[KillUnit] Error starting unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, "cannot kill unit")
	}
	return &systemd_api.UnitResponse{CmdStatus: "Command successful"}, nil
}

func (s *SystemdControlServer) FreezeUnit(ctx context.Context, req *systemd_api.UnitRequest) (*systemd_api.UnitResponse, error) {
	log.Infof("Incoming request to freeze %v", req)

	err := s.Controller.FreezeUnit(context.Background(), req.UnitName)
	if err != nil {
		log.Infof("[FreezeUnit] Error freezing unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, "cannot freeze unit")
	}
	return &systemd_api.UnitResponse{CmdStatus: "Command successful"}, nil
}

func (s *SystemdControlServer) UnfreezeUnit(ctx context.Context, req *systemd_api.UnitRequest) (*systemd_api.UnitResponse, error) {
	log.Infof("Incoming request to unfreeze %v\n", req)

	err := s.Controller.UnfreezeUnit(context.Background(), req.UnitName)
	if err != nil {
		log.Infof("[StartUnit] Error un-freezing unit: %v\n", err)
		return nil, grpc_status.Error(grpc_codes.Unknown, "cannot unfreeze unit")
	}
	return &systemd_api.UnitResponse{CmdStatus: "Command successful"}, nil
}

func (s *SystemdControlServer) MonitorUnit(req *systemd_api.UnitResourceRequest, stream systemd_api.UnitControlService_MonitorUnitServer) error {
	log.Infof("Incoming resource monitor request for %v\n", req)

	// Find unit
	units, err := s.Controller.FindUnit(req.UnitName)
	if err != nil {
		return grpc_status.Error(grpc_codes.NotFound, "cannot monitor unit")
	}
	if len(units) != 1 {
		return fmt.Errorf("none or more than one unit found")
	}
	unit := units[0]

	if unit.ActiveState != "active" {
		return fmt.Errorf("unit %s is %s", unit.Name, unit.ActiveState)
	}

	// Get pid from unit property or pid
	unitProps, err := s.Controller.GetUnitProperties(context.Background(), unit.Name)
	if err != nil {
		return err
	}

	pid, ok := unitProps["MainPID"].(uint32)
	if !ok || pid == 0 {
		return fmt.Errorf("failed to unwrap integer value from dbus.Variant")
	}

	for i := 0; i < 50; i += 1 {
		cpuUsage, memoryUsage, err := s.Controller.GetUnitCpuAndMem(context.Background(), pid)
		if err != nil {
			log.Infof("[MonitorUnit] Error fetching unit properties: %v\n", err)
			return fmt.Errorf("cannot fetch unit properties")
		}
		resp := &systemd_api.UnitResourceResponse{
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

func (s *SystemdControlServer) StartApplication(ctx context.Context, req *systemd_api.AppUnitRequest) (*systemd_api.UnitResponse, error) {
	log.Infof("Executing application start method for: %s\n", req.UnitName)
	resp, err := s.Controller.StartApplication(ctx, req.UnitName, req.Args)
	if err != nil {
		return nil, err
	}
	return &systemd_api.UnitResponse{CmdStatus: resp}, nil
}
