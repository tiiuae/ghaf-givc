// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package statsmanager

import (
	"context"
	"fmt"

	stats_api "givc/modules/api/stats"
	stats_msg "givc/modules/api/stats_message"

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

func (s *StatsServer) GetStats(ctx context.Context, req *stats_api.StatsRequest) (*stats_msg.StatsResponse, error) {
	log.Infof("Incoming request to get hardware identifier\n")

	memorystats, err := s.Controller.GetMemoryStats(context.Background())
	if err != nil {
		log.Infof("[GetStats] Error getting memory statistics: %v\n", err)
		return nil, fmt.Errorf("cannot get memory statistics")
	}

	return &stats_msg.StatsResponse{Memory: memorystats}, nil
}
