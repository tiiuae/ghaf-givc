// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package notifier

import (
	"context"
	givc_notifier "givc/modules/api/notify"

	"google.golang.org/grpc"
	"google.golang.org/protobuf/encoding/protojson"

	log "github.com/sirupsen/logrus"
)

type UserNotifierServer struct {
	socketController *SocketController
	givc_notifier.UnimplementedUserNotificationServiceServer
}

func (s *UserNotifierServer) Name() string {
	return "User Notifier"
}

func (s *UserNotifierServer) RegisterGrpcService(srv *grpc.Server) {
	givc_notifier.RegisterUserNotificationServiceServer(srv, s)
}

// NewUserNotifierServer
func NewUserNotifierServer(socket string) (*UserNotifierServer, error) {

	// Create a new user notifier controller
	var err error
	socketController, err := NewSocketController(socket)
	if err != nil {
		return nil, err
	}

	return &UserNotifierServer{
		socketController: socketController,
	}, nil
}

// NotifyUser sends a notification to the user desktop environment
func (s *UserNotifierServer) NotifyUser(ctx context.Context, notification *givc_notifier.UserNotification) (*givc_notifier.Status, error) {

	// Convert notification to JSON
	jsonData, err := protojson.Marshal(notification)
	if err != nil {
		log.Warnf("Failed to marshal notification to JSON: %v", err)
		return nil, err
	}

	// Broadcast notification to all user sockets
	err = s.socketController.BroadcastNotification(jsonData)
	if err != nil {
		log.Warnf("Error broadcasting notification: %v", err)
		return nil, err
	}

	status := &givc_notifier.Status{
		Status: "Notification sent",
	}
	log.Infof("Notification sent: %v", jsonData)

	return status, nil
}
