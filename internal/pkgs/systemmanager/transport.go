// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package systemmanager

import (
	context "context"
	"fmt"

	admin_api "givc/api/admin"
	"givc/internal/pkgs/types"

	log "github.com/sirupsen/logrus"
	grpc "google.golang.org/grpc"
)

// Admin Server
type AdminServer struct {
	AdminService *AdminService
	admin_api.UnimplementedAdminServiceServer
}

func (s *AdminServer) Name() string {
	return "Admin Server"
}
func (s *AdminServer) RegisterGrpcService(srv *grpc.Server) {
	admin_api.RegisterAdminServiceServer(srv, s)
}

func NewAdminServer(cfg *types.EndpointConfig) *AdminServer {
	return &AdminServer{
		AdminService: NewAdminService(cfg),
	}
}

func (s *AdminServer) RegisterService(ctx context.Context, req *admin_api.RegistryRequest) (*admin_api.RegistryResponse, error) {

	// @TODO verify remote

	// Create entry and register
	registryEntry := &types.RegistryEntry{
		Name:   req.Name,
		Parent: req.Parent,
		Type:   types.UnitType(req.Type),
		Transport: types.TransportConfig{
			Name:     req.Transport.Name,
			Address:  req.Transport.Address,
			Port:     req.Transport.Port,
			Protocol: req.Transport.Protocol,
		},
	}

	log.Infof("Registering service: %v", registryEntry)

	err := s.AdminService.RegisterService(registryEntry)
	if err != nil {
		log.Errorf("Error registering service: %v", err)
		err = fmt.Errorf("error registering service: %v", err)
		return nil, err
	}
	resp := &admin_api.RegistryResponse{CmdStatus: "Registration successful"}

	return resp, nil
}

func (s *AdminServer) Poweroff(ctx context.Context, req *admin_api.Empty) (*admin_api.Empty, error) {
	err := s.AdminService.Poweroff()
	if err != nil {
		log.Errorf(err.Error())
	}
	return &admin_api.Empty{}, err
}

func (s *AdminServer) Reboot(ctx context.Context, req *admin_api.Empty) (*admin_api.Empty, error) {
	err := s.AdminService.Reboot()
	if err != nil {
		log.Errorf(err.Error())
	}
	return &admin_api.Empty{}, err
}

func (s *AdminServer) StartApplication(ctx context.Context, req *admin_api.ApplicationRequest) (*admin_api.ApplicationResponse, error) {

	// @TODO verify remote

	var appStatus string
	cmdStatus := "Command success."
	appStatus, err := s.AdminService.StartApplication(req.AppName)
	if err != nil {
		cmdStatus = "Command failed"
		appStatus = err.Error()
	}

	resp := &admin_api.ApplicationResponse{
		CmdStatus: cmdStatus,
		AppStatus: appStatus,
	}

	return resp, nil
}

func (s *AdminServer) PauseApplication(ctx context.Context, req *admin_api.ApplicationRequest) (*admin_api.ApplicationResponse, error) {

	// @TODO verify remote

	var appStatus string
	cmdStatus := "Command success."
	appStatus, err := s.AdminService.PauseApplication(req.AppName)
	if err != nil {
		cmdStatus = "Command failed"
		appStatus = err.Error()
	}

	resp := &admin_api.ApplicationResponse{
		CmdStatus: cmdStatus,
		AppStatus: appStatus,
	}

	return resp, nil
}

func (s *AdminServer) ResumeApplication(ctx context.Context, req *admin_api.ApplicationRequest) (*admin_api.ApplicationResponse, error) {

	// @TODO verify remote

	cmdStatus := "terrific"
	appStatus := "yes"

	resp := &admin_api.ApplicationResponse{
		CmdStatus: cmdStatus,
		AppStatus: appStatus,
	}

	return resp, nil
}

func (s *AdminServer) StopApplication(ctx context.Context, req *admin_api.ApplicationRequest) (*admin_api.ApplicationResponse, error) {

	// @TODO verify remote

	cmdStatus := "terrific"
	appStatus := "yes"

	resp := &admin_api.ApplicationResponse{
		CmdStatus: cmdStatus,
		AppStatus: appStatus,
	}

	return resp, nil
}
