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
		log.Infof("[ListNetwork] Error fetching network list: %v\n", err)
		return nil, fmt.Errorf("cannot fetch network list")
	}

	resp := wifi_api.WifiNetworkResponse{
		Networks: networks,
	}

	return &resp, nil
}

func (s *WifiControlServer) GetActiveConnection(ctx context.Context, req *wifi_api.EmptyRequest) (*wifi_api.AccessPoint, error) {
	log.Infof("Incoming request to list available APs")

	connection, ssid, signal, security, err := s.Controller.GetActiveConnection(context.Background())
	if err != nil {
		log.Infof("[GetActiveConnection] Error fetching network list: %v\n", err)
		return nil, fmt.Errorf("cannot fetch network list")
	}

	resp := wifi_api.AccessPoint{
		Connection: connection,
		SSID:       ssid,
		Signal:     signal,
		Security:   security,
	}

	if !connection {
		log.Infof("[GetActiveConnection] No active connection\n")
	}

	return &resp, nil
}

func (s *WifiControlServer) ConnectNetwork(ctx context.Context, req *wifi_api.WifiConnectionRequest) (*wifi_api.WifiConnectionResponse, error) {
	log.Infof("Incoming connection request to %v\n", req.SSID)

	response, err := s.Controller.Connect(context.Background(), req.SSID, req.Password, req.Settings)
	if err != nil {
		log.Infof("[ConnectNetwork] Error AP connection: %v %v\n", response, err)
		return nil, fmt.Errorf("cannot connect to AP %s (%s)", response, err)
	}

	return &wifi_api.WifiConnectionResponse{Response: response}, nil
}

func (s *WifiControlServer) DisconnectNetwork(ctx context.Context, req *wifi_api.EmptyRequest) (*wifi_api.WifiConnectionResponse, error) {
	log.Infof("Incoming disconnection request\n")

	response, err := s.Controller.Disconnect(context.Background())
	if err != nil {
		log.Infof("[DisconnectNetwork] Error AP disconnection: %v %v\n", response, err)
		return nil, fmt.Errorf("cannot disconnect fromAP %s (%s)", response, err)
	}

	return &wifi_api.WifiConnectionResponse{Response: response}, nil
}

func (s *WifiControlServer) TurnOn(ctx context.Context, req *wifi_api.EmptyRequest) (*wifi_api.WifiConnectionResponse, error) {
	log.Infof("Incoming request to turn on the wifi\n")

	response, err := s.Controller.WifiRadioSwitch(context.Background(), true)
	if err != nil {
		log.Infof("[TurnOn] Error switching the network: %v\n", err)
		return nil, fmt.Errorf("cannot switch the network")
	}

	return &wifi_api.WifiConnectionResponse{Response: response}, nil
}

func (s *WifiControlServer) TurnOff(ctx context.Context, req *wifi_api.EmptyRequest) (*wifi_api.WifiConnectionResponse, error) {
	log.Infof("Incoming request to turn off the wifi\n")

	response, err := s.Controller.WifiRadioSwitch(context.Background(), false)
	if err != nil {
		log.Infof("[TurnOff] Error switching the network: %v\n", err)
		return nil, fmt.Errorf("cannot switch the network")
	}

	return &wifi_api.WifiConnectionResponse{Response: response}, nil
}
