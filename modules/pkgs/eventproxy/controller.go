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

	givc_types "givc/modules/pkgs/types"

	"github.com/jbdemonte/virtual-device/gamepad"
	"github.com/jochenvg/go-udev"

	log "github.com/sirupsen/logrus"
)

type EventProxyController struct {
	runAsProducer   bool
	transportConfig givc_types.TransportConfig
	gameDevice      gamepad.VirtualGamepad
}

// NewEventProxyController creates a new EventProxyController instance.
// It sets up a tcp connection for communication.
func NewEventProxyController(transportConfig givc_types.TransportConfig, runAsProducer bool) (*EventProxyController, error) {

	var err error
	var device gamepad.VirtualGamepad
	if !runAsProducer {

		// Create a virtual gamepad device
		// TODO: Extract DeviceID and VendorID and create device based on that
		device = gamepad.NewXBox360()
		if err != nil {
			log.Errorf("event: error in creating gamepad device: %v", err)
		}

		err = device.Register()
		if err != nil {
			log.Errorf("event: failed to register gamepad device: %v", err)
		}
	}
	return &EventProxyController{runAsProducer: runAsProducer, transportConfig: transportConfig, gameDevice: device}, err
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

func (s *EventProxyController) findDevice() string {
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
			log.Infof("event: device node found is: ", deviceNode)
			break
		}
	}
	return deviceNode
}

func (s *EventProxyController) monitorInputDevices() string {
	log.Infof("event: Monitoring UEvent kernel message to user-space")
	// Add filters to monitor
	userDevices := udev.Udev{}
	monitor := userDevices.NewMonitorFromNetlink("udev")
	monitor.FilterAddMatchSubsystemDevtype("input", "event")
	monitor.FilterAddMatchTag("")

	var deviceSysPath string
	var deviceNode string

	// Create a context
	ctx, cancel := context.WithCancel(context.Background())

	// Start monitor goroutine and get receive channel
	channel, _, _ := monitor.DeviceChan(ctx)

	// WaitGroup for timers
	var wg sync.WaitGroup
	wg.Add(2)
	go func() {
		log.Infof("event: started listening on channel")
		for device := range channel {
			if device.Action() == "add" {
				// Monitor for the wireless controller device
				if strings.Contains(strings.ToLower(device.PropertyValue("NAME")), "wireless controller") {
					deviceSysPath = device.Syspath()
				} else if strings.Contains(device.Syspath(), deviceSysPath) && strings.Contains(device.Sysname(), "event") {
					// Extract the device node for wireless controller event
					deviceNode = device.Devnode() // device node located in /dev/input/eventX
					cancel()
					log.Infof("event: device channel closed!")
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

	return deviceNode
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
