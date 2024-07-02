// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package wifimanager

import (
	"context"
	"fmt"

	"time"

	wifi_api "givc/api/wifi"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

const (
	ResourceStreamInterval = 400 * time.Millisecond
)

type WifiControlServer struct {
	Controller *WifiController
	wifi_api.UnimplementedWifiServiceServer
}

func (s *WifiControlServer) Name() string {
	return "Wifi Control Server"
}

func (s *WifiControlServer) RegisterGrpcService(srv *grpc.Server) {
	wifi_api.RegisterWifiServiceServer(srv, s)
}

func NewWifiControlServer() (*WifiControlServer, error) {

	wifiController, err := NewController()
	if err != nil {
		log.Errorf("Error creating wifi controller: %v", err)
		return nil, err
	}

	wifiControlServer := WifiControlServer{
		Controller: wifiController,
	}

	return &wifiControlServer, nil
}

func (s *WifiControlServer) ListNetwork(ctx context.Context, req *wifi_api.WifiNetworkRequest) (*wifi_api.WifiNetworkResponse, error) {
	log.Infof("Incoming request to list available APs of %v\n", req)

	networks, err := s.Controller.GetNetworkList(context.Background(), req.NetworkName)
	if err != nil {
		log.Infof("[ListNetwork] Error fetching network properties: %v\n", err)
		return nil, fmt.Errorf("cannot fetch network properties")
	}

	resp := wifi_api.WifiNetworkResponse{
		InUse:    networks["IN-USE"],
		SSID:     networks["SSID"],
		Signal:   networks["SIGNAL"],
		Security: networks["SECURITY"],
	}

	return &resp, nil
}

func (s *WifiControlServer) ConnectNetwork(ctx context.Context, req *wifi_api.WifiConnectionRequest) (*wifi_api.WifiConnectionResponse, error) {
	log.Infof("Incoming request to connect %v\n", req)

	response, err := s.Controller.Connect(context.Background(), req.SSID, req.Password)
	if err != nil {
		log.Infof("[ConnectNetwork] Error fetching network properties: %v %v\n", response, err)
		return nil, fmt.Errorf("cannot fetch network properties %s (%s)", response, err)
	}

	return &wifi_api.WifiConnectionResponse{Response: response}, nil
}
