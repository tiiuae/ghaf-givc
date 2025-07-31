// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package eventproxy

import (
	"context"
	"errors"
	"strings"
	"syscall"
	"time"

	givc_event "givc/modules/api/event"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_types "givc/modules/pkgs/types"

	"github.com/holoplot/go-evdev"
	"github.com/jbdemonte/virtual-device/gamepad"
	"google.golang.org/grpc"

	log "github.com/sirupsen/logrus"
)

type EventProxyServer struct {
	eventController *EventProxyController
	givc_event.UnimplementedEventServiceServer
}

func (s *EventProxyServer) StreamEvents(stream givc_event.EventService_StreamEventsServer) error {

	deviceNode := s.eventController.FindDevice()
	dev, err := evdev.Open(deviceNode)
	if err != nil {
		log.Errorf("event: failed to open input device: %v", err)
	}

	for {
		event, err := stream.Recv()

		if err != nil {
			stream.SendAndClose(&givc_event.Ack{Status: "Stream Error"})
			return err
		}

		log.Infof("event: received InputEvent: type=%v code=%v value=%v", event.Type, event.Code, event.Value)

		timeStamp := time.Unix(0, event.Timestamp)
		time := syscall.Timeval{
			Sec:  int64(timeStamp.Unix()), // seconds since epoch
			Usec: int64(timeStamp.Nanosecond() / 1000),
		}

		err = dev.WriteOne(&evdev.InputEvent{
			Time:  time,
			Type:  evdev.EvType(event.Type),
			Code:  evdev.EvCode(event.Code),
			Value: event.Value,
		})
		if err != nil {
			log.Fatalf("event: failed to write InputEvent: %v", err)
		}
	}
}

func (s *EventProxyServer) Name() string {
	return "Event Proxy Server"
}

func (s *EventProxyServer) RegisterGrpcService(srv *grpc.Server) {
	givc_event.RegisterEventServiceServer(srv, s)
}

func (s *EventProxyServer) Close() error {
	return s.eventController.Close()
}

func NewEventProxyServer(transport givc_types.TransportConfig) (*EventProxyServer, error) {

	// Create a new event proxy controller
	var err error
	eventController, err := NewEventProxyController(transport)
	if err != nil {
		return nil, err
	}

	return &EventProxyServer{
		eventController: eventController,
	}, nil
}

func (s *EventProxyServer) RegisterDevice(ctx context.Context, info *givc_event.DeviceInfo) (*givc_event.Ack, error) {

	if strings.Contains(strings.ToLower(info.Name), "wireless controller") {
		device := gamepad.NewXBox360()

		err := device.Register()
		if err != nil {
			log.Errorf("event: failed to register gamepad device: %v", err)
		}
		log.Infof("event: registered device %s with VendorID:0x%x DeviceID:0x%x", info.Name, info.VendorId, info.DeviceId)
		s.eventController.gameDevice = device
	} else {
		log.Infof("event: device %s is not supported!", info.Name)
	}

	return &givc_event.Ack{Status: "0"}, nil
}

func (s *EventProxyServer) StreamEventsToRemote(ctx context.Context, cfg *givc_types.EndpointConfig, device string) error {

	defer s.eventController.Close()

	// Setup GRPC client
	grpcClientConn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		log.Errorf("event: Error in getting grpc client %v", err)
		return err
	}
	defer grpcClientConn.Close()

	// Create streaming client for the events
	eventStreamClient := givc_event.NewEventServiceClient(grpcClientConn)
	if eventStreamClient == nil {
		return errors.New("event: failed to create 'NewEventServiceClient'")
	}

	// Monitor for Input device provided by the user
	handler, err := s.eventController.MonitorInputDevice(device)
	if err != nil || handler == "" {
		log.Fatalf("event: failed to monitor channel %v", err)
		return err
	}
	dev, err := evdev.Open(handler)
	if err != nil {
		log.Errorf("event: failed to open input device: %v", err)
	}
	defer dev.Close()

	deviceInfo, err := s.eventController.ExtractDeviceInfo(dev)
	if err != nil {
		log.Fatalf("event: failed to extract device info: %v", err)
	}

	for {
		select {
		// Return if context is done
		case <-ctx.Done():
			return nil

		default:

			s.eventController.WaitForConsumer()
			if err != nil {
				return err
			}

			_, err = eventStreamClient.RegisterDevice(ctx, deviceInfo)
			if err != nil {
				log.Fatalf("event: failed to register device: %v", err)
			}

			stream, err := eventStreamClient.StreamEvents(ctx)
			for err != nil {
				time.Sleep(1 * time.Second)
				stream, err = eventStreamClient.StreamEvents(ctx)
			}

			if err == nil {
				for {
					event, err := dev.ReadOne()
					if err != nil {
						if strings.Contains(err.Error(), "no such device") {
							return errors.New("event: device not available, possibly device is disconnected")
						}
						time.Sleep(1 * time.Second)
						continue
					}

					ts := time.Unix(event.Time.Sec, int64(event.Time.Usec)*1000).UnixNano()

					// Stream Input Events received to remote
					msg := &givc_event.InputEvent{
						Timestamp: ts,
						Type:      uint32(event.Type),
						Code:      uint32(event.Code),
						Value:     event.Value,
					}

					log.Infof("event: sending InputEvent: type=%v code=%v value=%v", event.Type, event.Code, event.Value)
					if err := stream.Send(msg); err != nil {
						log.Fatalf("event: failed to send InputEvent: %v", err)
					}
				}
			}

			// Close stream connection
			if stream != nil {
				err := stream.CloseSend()
				if err != nil {
					log.Warnf("event: Error closing stream: %v", err)
				}
			}
		}
	}
}
