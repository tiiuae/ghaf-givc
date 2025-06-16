// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// The statsmanager package provides functionality to manage and retrieve system statistics.
package statsmanager

import (
	"context"
	"fmt"

	stats_api "givc/modules/api/stats"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type StatsServer struct {
	Controller *StatsController
	stats_api.UnimplementedStatsServiceServer
}

func (s *StatsServer) Name() string {
	return "Statistics Server"
}

func (s *StatsServer) RegisterGrpcService(srv *grpc.Server) {
	stats_api.RegisterStatsServiceServer(srv, s)
}

// NewStatsServer creates a new instance of StatsServer with the provided service whitelist and applications.
func NewStatsServer() (*StatsServer, error) {

	statsController, err := NewController()
	if err != nil {
		log.Errorf("Error creating stats controller: %v", err)
		return nil, err
	}

	statsServer := StatsServer{
		Controller: statsController,
	}

	return &statsServer, nil
}

// GetStats handles incoming requests for system statistics.
func (s *StatsServer) GetStats(ctx context.Context, req *stats_api.StatsRequest) (*stats_api.StatsResponse, error) {
	log.Infof("Incoming request to get statistics\n")

	memorystats, err := s.Controller.GetMemoryStats(context.Background())
	if err != nil {
		log.Infof("[GetStats] Error getting memory statistics: %v\n", err)
		return nil, fmt.Errorf("cannot get memory statistics")
	}

	loadstats, err := s.Controller.GetLoadStats(context.Background())
	if err != nil {
		log.Infof("[GetStats] Error getting load statistics: %v\n", err)
		return nil, fmt.Errorf("cannot get load statistics")
	}

	processstats, err := s.Controller.GetProcessStats(context.Background())
	if err != nil {
		log.Infof("[GetStats] Error getting process statistics: %v\n", err)
		return nil, fmt.Errorf("cannot get process statistics")
	}

	return &stats_api.StatsResponse{
		Memory:  memorystats,
		Load:    loadstats,
		Process: processstats,
	}, nil
}
