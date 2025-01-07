// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package hwidmanager

import (
	"context"
	"fmt"

	hwid "givc/modules/api/hwid"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type HwIdServer struct {
	Controller *HwIdController
	hwid.UnimplementedHwidServiceServer
}

func (s *HwIdServer) Name() string {
	return "Hardware Identifier Server"
}

func (s *HwIdServer) RegisterGrpcService(srv *grpc.Server) {
	hwid.RegisterHwidServiceServer(srv, s)
}

func NewHwIdServer(iface string) (*HwIdServer, error) {

	hwidController, err := NewController(iface)
	if err != nil {
		log.Errorf("Error creating hwid controller: %v", err)
		return nil, err
	}

	hwidServer := HwIdServer{
		Controller: hwidController,
	}

	return &hwidServer, nil
}

func (s *HwIdServer) GetHwId(ctx context.Context, req *hwid.HwIdRequest) (*hwid.HwIdResponse, error) {
	log.Infof("Incoming request to get hardware identifier\n")

	identifier, err := s.Controller.GetIdentifier(context.Background())
	if err != nil {
		log.Infof("[GetHwId] Error getting hardware identifier: %v\n", err)
		return nil, fmt.Errorf("cannot get hardware id")
	}

	return &hwid.HwIdResponse{Identifier: identifier}, nil
}
