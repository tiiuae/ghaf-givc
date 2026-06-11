// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package vm

import (
	"context"
	"fmt"

	"time"

	givc_stats "givc/modules/api/stats"
	givc_vm "givc/modules/api/vm"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

const (
	ResourceStreamInterval = 400 * time.Millisecond
)

type VmControlServer struct {
	Controller *VmController
	givc_vm.UnimplementedVMServiceServer
}

func (s *VmControlServer) Name() string {
	return "Vm Control Server"
}

func (s *VmControlServer) RegisterGrpcService(srv *grpc.Server) {
	givc_vm.RegisterVMServiceServer(srv, s)
}

func NewVmControlServer() (*VmControlServer, error) {

	vmController, err := NewController()
	if err != nil {
		log.Errorf("Error creating vm controller: %v", err)
		return nil, err
	}

	vmControlServer := VmControlServer{
		Controller: vmController,
	}

	return &vmControlServer, nil
}

func (s *VmControlServer) SetVMSize(ctx context.Context, req *givc_vm.VMSizeRequest) (*givc_vm.VMSizeResponse, error) {
	log.Infof("Incoming disconnection request\n")

	ret, err := s.Controller.VMSize(context.Background(), req.Vm, req.Minimum, req.Maximum)
	if err != nil {
		log.Infof("[DisconnectNetwork] Error AP disconnection: %v\n", err)
		return nil, fmt.Errorf("cannot disconnect fromAP (%v)", err)
	}

	return ret, nil
}

func (s *VmControlServer) GetStats(ctx context.Context, req *givc_vm.VMStatsRequest) (*givc_stats.MemoryStats, error) {
	statsp, err := s.Controller.VMStats(ctx, req.Name)
	if err != nil {
		return nil, err
	}
	stats := *statsp

	total, ok := stats["BalloonSize"]
	if !ok {
		return nil, fmt.Errorf("Missing data")
	}
	free, ok := stats["FreeMemory"]
	if !ok {
		return nil, fmt.Errorf("Missing data")
	}
	available, ok := stats["AvailableMemory"]
	if !ok {
		return nil, fmt.Errorf("Missing data")
	}

	return &givc_stats.MemoryStats{Total: total, Free: free, Available: available}, nil
}
