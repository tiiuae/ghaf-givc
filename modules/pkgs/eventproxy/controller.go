// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package eventproxy

import (
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"

	givc_event "givc/modules/api/event"
	givc_types "givc/modules/pkgs/types"

	"github.com/fsnotify/fsnotify"
	evdev "github.com/holoplot/go-evdev"
	"github.com/jbdemonte/virtual-device/gamepad"
	"github.com/jbdemonte/virtual-device/mouse"
	"golang.org/x/sys/unix"

	log "github.com/sirupsen/logrus"
)

type EventProxyController struct {
	transportConfig givc_types.TransportConfig
	virtualGamepad  gamepad.VirtualGamepad
	virtualMouse    mouse.VirtualMouse
}

// NewEventProxyController creates a new EventProxyController instance.
// It sets up a tcp connection for communication.
func NewEventProxyController(transportConfig givc_types.TransportConfig) (*EventProxyController, error) {
	return &EventProxyController{transportConfig: transportConfig}, nil
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

func parseUdevDeviceType(data string) string {
	lines := strings.Split(data, "\n")
	var isMouse, isGamepad, isKeyboard, isJoystick bool
	for _, line := range lines {
		switch {
		case strings.HasPrefix(line, "E:ID_INPUT_MOUSE=1"):
			isMouse = true
		case strings.HasPrefix(line, "E:ID_INPUT_JOYSTICK=1"):
			isJoystick = true
		case strings.HasPrefix(line, "E:ID_INPUT_GAMEPAD=1"):
			isGamepad = true
		case strings.HasPrefix(line, "E:ID_INPUT_KEYBOARD=1"):
			isKeyboard = true
		}
	}

	if isMouse {
		return "mouse"
	}
	if isJoystick {
		return "joystick"
	}
	if isGamepad {
		return "gamepad"
	}
	if isKeyboard {
		return "keyboard"
	}

	return ""
}

func getDeviceType(devicePath string, waitTimeout time.Duration) string {
	var stat syscall.Stat_t

	if err := syscall.Stat(devicePath, &stat); err != nil {
		return ""
	}

	major := unix.Major(stat.Rdev)
	minor := unix.Minor(stat.Rdev)
	udevFile := fmt.Sprintf("/run/udev/data/c%d:%d", major, minor)

	var data []byte
	var err error
	deadline := time.Now().Add(waitTimeout)
	for {
		data, err = os.ReadFile(udevFile)
		if err == nil {
			break
		}
		if time.Now().After(deadline) {
			break
		}
		time.Sleep(50 * time.Millisecond)
	}

	if err != nil {
		return ""
	}

	return parseUdevDeviceType(string(data))
}

func extractDeviceInfo(dev *evdev.InputDevice) (*givc_event.DeviceInfo, error) {

	inputID, err := dev.InputID()
	if err != nil {
		return nil, err
	}

	name, err := dev.Name()
	if err != nil {
		return nil, err
	}

	deviceType := getDeviceType(dev.Path(), 1*time.Second)
	if deviceType == "" {
		log.Errorf("event: device %s not supported, unknown device type!", name)
		return nil, fmt.Errorf("event: device %s not supported, unknown device type!", name)
	}

	log.Infof("event: device %s detected, device type: %s", name, deviceType)

	deviceInfo := &givc_event.DeviceInfo{
		VendorId:   uint32(inputID.Vendor),
		DeviceId:   uint32(inputID.Product),
		Name:       name,
		DeviceType: deviceType,
	}
	return deviceInfo, nil
}

func (s *EventProxyController) OpenAndExtract(handler string) (*evdev.InputDevice, *givc_event.DeviceInfo, error) {
	dev, err := evdev.Open(handler)
	if err != nil {
		log.Errorf("event: failed to open input device: %v", err)
		return nil, nil, err
	}

	deviceInfo, err := extractDeviceInfo(dev)
	if err != nil {
		dev.Close() // clean up on failure
		log.Errorf("event: failed to extract device info: %v", err)
		return nil, nil, err
	}

	return dev, deviceInfo, nil
}

func checkDevice(targetDevice string, event string, waitTimeout time.Duration) bool {
	deviceType := getDeviceType(event, waitTimeout)
	if deviceType == "" || !strings.EqualFold(deviceType, targetDevice) {
		return false
	}

	dev, err := evdev.Open(event)
	if err != nil {
		return false
	}
	name, _ := dev.Name()
	dev.Close()

	if strings.Contains(strings.ToLower(name), "vmmouse") {
		log.Debugf("event: ignoring device: %s", name)
		return false
	}

	log.Infof("event: device attached by type %s: %s", deviceType, name)
	return true
}

func (s *EventProxyController) MonitorInputDevices(targetDevice string, onAdd func(string)) (string, error) {
	log.Infof("event: Monitoring started for device: %s", targetDevice)
	// Create a channel to signal when the target device is found
	done := make(chan struct{})

	var once sync.Once
	var handler string

	watcher, err := fsnotify.NewWatcher()
	if err != nil {
		return "", fmt.Errorf("event: failed to create watcher %w", err)
	}
	defer watcher.Close()

	inputDir := "/dev/input"
	if err := watcher.Add(inputDir); err != nil {
		return "", fmt.Errorf("event: failed to watch %s: %w", inputDir, err)
	}

	// Check existing devices at start
	filepath.Walk(inputDir, func(path string, info os.FileInfo, err error) error {
		if err == nil && !info.IsDir() && info.Mode()&os.ModeCharDevice != 0 && strings.HasPrefix(filepath.Base(path), "event") {
			if checkDevice(targetDevice, path, 0) {
				once.Do(func() {
					handler = path
					close(done)
				})
				return nil
			}
			onAdd(path)
		}
		return nil
	})

	go func() {
		for {
			select {
			case event, ok := <-watcher.Events:
				if !ok {
					return
				}
				if event.Op&fsnotify.Create == fsnotify.Create && strings.HasPrefix(filepath.Base(event.Name), "event") {
					go func(devicePath string) {
						if checkDevice(targetDevice, devicePath, 1*time.Second) {
							once.Do(func() {
								handler = devicePath
								close(done)
							})
							return
						}
						onAdd(devicePath)
					}(event.Name)
				}
			case err, ok := <-watcher.Errors:
				if !ok {
					return
				}
				log.Errorf("event: watch error %v", err)
			}
		}
	}()
	<-done // Wait until the target device is found or an error occurs
	return handler, nil
}

// Closes the socket listener.
func (s *EventProxyController) Close() error {
	if s.virtualGamepad != nil {
		err := s.virtualGamepad.Unregister()
		if err != nil {
			return err
		}
	} else if s.virtualMouse != nil {
		err := s.virtualMouse.Unregister()
		if err != nil {
			return err
		}
	}
	return nil
}
