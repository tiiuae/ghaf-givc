// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package eventproxy

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	givc_event "givc/modules/api/event"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_types "givc/modules/pkgs/types"

	"github.com/holoplot/go-evdev"
	"google.golang.org/grpc"

	log "github.com/sirupsen/logrus"
)

type EventProxyServer struct {
	eventController *EventProxyController
	givc_event.UnimplementedEventServiceServer
}

func (s *EventProxyServer) StreamEvents(stream givc_event.EventService_StreamEventsServer) error {

	for {
		ev, err := stream.Recv()

		if err != nil {
			return err
		}

		log.Infof("event: type=%d code=%d value=%d\n", ev.Type, ev.Code, ev.Value)

		switch ev.Code {
		case 0: // ABS_X
			s.eventController.gameDevice.MoveLeftStickX(float32(ev.Value))
		case 1: // ABS_Y
			s.eventController.gameDevice.MoveLeftStickY(float32(ev.Value))
		case 3: // ABS_RX
			s.eventController.gameDevice.MoveRightStickX(float32(ev.Value))
		case 4: // ABS_RY
			s.eventController.gameDevice.MoveRightStickY(float32(ev.Value))
		default:
			log.Warnf("event: unhandled abs code: %d\n", ev.Code)
		}
	}
	return stream.SendAndClose(&givc_event.Ack{Status: "ok"})

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

func NewEventProxyServer(transport givc_types.TransportConfig, runAsProducer bool) (*EventProxyServer, error) {

	// Create a new event proxy controller
	var err error
	eventController, err := NewEventProxyController(transport, runAsProducer)
	if err != nil {
		return nil, err
	}

	return &EventProxyServer{
		eventController: eventController,
	}, nil
}

func (s *EventProxyServer) StreamEventsToRemote(ctx context.Context, cfg *givc_types.EndpointConfig) error {

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
		return fmt.Errorf("event: failed to create 'NewEventServiceClient'")
	}

	deviceNode := s.eventController.monitorInputDevices()

	for {
		select {
		// Return if context is done
		case <-ctx.Done():
			return nil

		// Stream data to remote
		default:

			// Wait for consumer for the input events
			s.eventController.WaitForConsumer()
			if err != nil {
				return err
			}

			stream, err := eventStreamClient.StreamEvents(ctx)
			for err != nil {
				time.Sleep(1 * time.Second)
				stream, err = eventStreamClient.StreamEvents(ctx)
			}

			if err == nil && deviceNode != "" {
				dev, err := evdev.Open(deviceNode)
				if err != nil {
					log.Errorf("event: failed to open input device: %v", err)
				}
				defer dev.Close()

				for {
					event, err := dev.ReadOne()
					if err != nil {
						if strings.Contains(err.Error(), "no such device") {
							return errors.New("event: device not available, possibly device is disconnected")
						}
						time.Sleep(1 * time.Second)
						continue
					}

					ts := time.Unix(event.Time.Sec, int64(event.Time.Usec)*1000)

					if event.Type == evdev.EV_ABS || event.Type == evdev.EV_KEY {
						msg := &givc_event.InputEvent{
							Timestamp: ts.UnixNano(),
							Type:      uint32(event.Type),
							Code:      uint32(event.Code),
							Value:     event.Value,
						}

						log.Infof("event: now sending input events over grpc. type=%v code=%v value=%v", event.Type, event.Code, event.Value)
						if err := stream.Send(msg); err != nil {
							log.Fatalf("event: failed to send InputEvent: %v", err)
						}
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
