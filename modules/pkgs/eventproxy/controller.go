// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package eventproxy

import (
	"bufio"
	"context"
	"net"
	"os"
	"strings"
	"sync"
	"time"

	givc_event "givc/modules/api/event"
	givc_types "givc/modules/pkgs/types"

	"github.com/holoplot/go-evdev"
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

// TODO: No need to findDevice explicitly once https://github.com/jbdemonte/virtual-device/pull/4 is merged
func (s *EventProxyController) FindDevice() string {
	file, err := os.Open("/proc/bus/input/devices")
	if err != nil {
		panic(err)
	}
	defer file.Close()
	scanner := bufio.NewScanner(file)

	var currentName string
	var handlers []string
	var deviceNode string

	for scanner.Scan() {
		line := scanner.Text()

		if strings.HasPrefix(line, "N: Name=") {
			currentName = strings.Trim(line[8:], `"`)
		}

		if strings.HasPrefix(line, "H: Handlers=") && currentName == "Xbox 360 Wireless Receiver (XBOX)" {
			fields := strings.Fields(line[12:])
			for _, f := range fields {
				if strings.HasPrefix(f, "event") {
					handlers = append(handlers, f)
				}
			}
		}
	}

	if len(handlers) > 0 {
		for _, h := range handlers {
			deviceNode = "/dev/input/" + h
			log.Infof("event: device node found is: %s", deviceNode)
			break
		}
	}
	return deviceNode
}

func (s *EventProxyController) MonitorInputDevice(deviceName string) (string, error) {

	log.Infof("event: Monitoring UEvent kernel message to user-space")
	// Add filters to monitor
	userDevices := udev.Udev{}
	monitor := userDevices.NewMonitorFromNetlink("udev")
	monitor.FilterAddMatchSubsystemDevtype("input", "event")
	monitor.FilterAddMatchTag("")

	var deviceSysPath, handler string

	// Create a context
	ctx, cancel := context.WithCancel(context.Background())

	// Start monitor goroutine and get receive channel
	channel, _, err := monitor.DeviceChan(ctx)
	if err != nil {
		return "", err
	}

	// WaitGroup for timers
	var wg sync.WaitGroup
	wg.Add(2)
	go func() {
		log.Infof("event: started listening on channel")
		for device := range channel {
			if device.Action() == "add" {
				// Monitor for the deviceName provided
				if strings.Contains(strings.ToLower(device.PropertyValue("NAME")), deviceName) {
					deviceSysPath = device.Syspath()
				} else if strings.Contains(device.Syspath(), deviceSysPath) && strings.Contains(device.Sysname(), "event") {
					// Extract the device node for wireless controller event
					handler = device.Devnode() // device node located in /dev/input/eventX
					cancel()
					log.Infof("event: device %s attached!", deviceName)
					wg.Done()
				}
			}
		}
	}()
	go func() {
		<-time.After(2 * time.Second)
		monitor.FilterRemove()
		monitor.FilterUpdate()
		wg.Done()
	}()
	wg.Wait()
	return handler, nil
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
