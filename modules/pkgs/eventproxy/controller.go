// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package eventproxy

import (
	"context"
	"fmt"
	"net"
	"strings"
	"time"

	givc_event "givc/modules/api/event"
	givc_types "givc/modules/pkgs/types"

	evdev "github.com/holoplot/go-evdev"
	"github.com/jbdemonte/virtual-device/gamepad"
	"github.com/jochenvg/go-udev"

	log "github.com/sirupsen/logrus"
)

type EventProxyController struct {
	transportConfig givc_types.TransportConfig
	gameDevice      gamepad.VirtualGamepad
}

// NewEventProxyController creates a new EventProxyController instance.
// It sets up a tcp connection for communication.
func NewEventProxyController(transportConfig givc_types.TransportConfig) (*EventProxyController, error) {
	return &EventProxyController{transportConfig: transportConfig, gameDevice: nil}, nil
}

func (s *EventProxyController) WaitForConsumer() (net.Conn, error) {
	deadline := time.Now().Add(60 * time.Second)
	var conn net.Conn
	var err error
	addr := s.transportConfig.Address + ":" + s.transportConfig.Port

	for time.Now().Before(deadline) {
		conn, err = net.Dial(s.transportConfig.Protocol, addr)
		if err == nil {
			log.Infof("event: successfully connected to consumer.")
			return conn, nil
		}
		log.Infof("event: consumer not ready, retrying in 1 second...")
		time.Sleep(1 * time.Second)
	}
	return nil, err
}

func (s *EventProxyController) ExtractDeviceInfo(dev *evdev.InputDevice) (*givc_event.DeviceInfo, error) {

	inputID, err := dev.InputID()
	if err != nil {
		return nil, err
	}

	name, err := dev.Name()
	if err != nil {
		return nil, err
	}

	deviceInfo := &givc_event.DeviceInfo{
		VendorId: uint32(inputID.Vendor),
		DeviceId: uint32(inputID.Product),
		Name:     name,
	}
	return deviceInfo, nil
}

func ExtractEvent(deviceName string) string {
	var event string

	devicePaths, err := evdev.ListDevicePaths()
	if err == nil {
		for _, dev := range devicePaths {
			// Check for the deviceName provided
			if strings.Contains(strings.ToLower(dev.Name), deviceName) {
				event = dev.Path // Located in /dev/input/eventX
				log.Infof("event: device %s attached!", dev.Name)
			}
		}
	}
	return event
}

func (s *EventProxyController) MonitorInputDevice(deviceName string) (string, error) {

	// Create a context with cancellation
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	// Add filters to monitor
	userDevices := udev.Udev{}
	monitor := userDevices.NewMonitorFromNetlink("udev")
	if err := monitor.FilterAddMatchSubsystemDevtype("input", "event"); err != nil {
		return "", err
	}

	// Start monitor goroutine and get receive channel
	channel, _, err := monitor.DeviceChan(ctx)
	if err != nil {
		return "", err
	}

	// Channel to communicate the found event
	eventChan := make(chan string, 1)

	// Goroutine to monitor devices
	go func() {
		defer close(eventChan)

		for {
			select {
			case <-ctx.Done():
				return // exit goroutine if canceled
			case device, ok := <-channel:
				if !ok {
					return // channel closed
				}
				if device.Action() == "add" && strings.Contains(device.Sysname(), "event") {
					if event := ExtractEvent(deviceName); event != "" {
						eventChan <- event
						return
					}
				}
			}
		}
	}()
	go func() {
		time.Sleep(2 * time.Second)
		if event := ExtractEvent(deviceName); event != "" {
			select {
			case eventChan <- event:
			default:
			}
		}
	}()

	// Wait for event or context timeout
	select {
	case event := <-eventChan:
		return event, nil
	case <-ctx.Done():
		return "", fmt.Errorf("device monitoring timed out")
	}
}

// Closes the socket listener.
func (s *EventProxyController) Close() error {
	if s.gameDevice != nil {
		err := s.gameDevice.Unregister()
		if err != nil {
			return err
		}
	}
	return nil
}
