// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package wifimanager

import (
	"context"
	"fmt"

	"github.com/godbus/dbus/v5"
)

type WifiController struct {
}

type WifiNetworkResponse struct {
	Ssid     []string
	Signal   []uint32
	Security []uint32
}

const (
	NM_ACTIVE_CONNECTION_STATE_UNKNOWN uint32 = iota
	NM_ACTIVE_CONNECTION_STATE_ACTIVATING
	NM_ACTIVE_CONNECTION_STATE_ACTIVATED
	NM_ACTIVE_CONNECTION_STATE_DEACTIVATING
	NM_ACTIVE_CONNECTION_STATE_DEACTIVATED
)

func NewController() (*WifiController, error) {
	return &WifiController{}, nil
}

func (c *WifiController) GetNetworkList(ctx context.Context, NetworkInterface string) (WifiNetworkResponse, error) {
	var devicePaths []dbus.ObjectPath
	var output WifiNetworkResponse

	// Input validation
	if ctx == nil {
		return output, fmt.Errorf("context cannot be nil")
	}

	conn, err := dbus.ConnectSystemBus()
	if err != nil {
		return output, fmt.Errorf("failed to connect to system bus: %s", err)
	}
	defer conn.Close()

	obj := conn.Object("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager")
	err = obj.Call("org.freedesktop.NetworkManager.GetDevices", 0).Store(&devicePaths)
	if err != nil {
		return output, fmt.Errorf("failed to get devices: %s", err)
	}

	// Iterate over devices to find Wi-Fi devices
	for _, devicePath := range devicePaths {
		var deviceType uint32

		device := conn.Object("org.freedesktop.NetworkManager", devicePath)
		deviceTypeVriant, err := device.GetProperty("org.freedesktop.NetworkManager.Device.DeviceType")
		if err != nil {
			return output, fmt.Errorf("failed to get device type: %s", err)
		}
		err = deviceTypeVriant.Store(&deviceType)
		if err != nil {
			return output, fmt.Errorf("failed to convert device type: %s", err)
		}

		// Check if the device is a Wi-Fi device (type 2)
		if deviceType == 2 {
			var apPaths []dbus.ObjectPath
			err = device.Call("org.freedesktop.NetworkManager.Device.Wireless.GetAllAccessPoints", 0).Store(&apPaths)
			if err != nil {
				return output, fmt.Errorf("failed to get access points: %s", err)
			}
			// Iterate over access points and append into output
			for _, apPath := range apPaths {
				var ssid []byte
				var strength uint32
				var wpaFlags, rsnFlags uint32

				ap := conn.Object("org.freedesktop.NetworkManager", apPath)
				ssid_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.Ssid")
				ssid_variant.Store(&ssid)
				if err != nil {
					return output, fmt.Errorf("failed to get SSID: %s", err)
				}
				strength_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.Strength")
				strength_variant.Store(&strength)
				if err != nil {
					return output, fmt.Errorf("failed to get Strength: %s", err)
				}
				wpaFlags_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.WpaFlags")
				wpaFlags_variant.Store(&wpaFlags)
				if err != nil {
					return output, fmt.Errorf("failed to get WPA flags: %s", err)
				}
				rsnFlags_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.RsnFlags")
				rsnFlags_variant.Store(&rsnFlags)
				if err != nil {
					return output, fmt.Errorf("failed to get RSN flags: %s", err)
				}
				// Convert variables to string and append it into output
				output.Ssid = append(output.Ssid, string(ssid))
				output.Signal = append(output.Signal, strength)
				output.Security = append(output.Security, wpaFlags+rsnFlags)
			}
		}
	}
	return output, nil
}

func (c *WifiController) Connect(ctx context.Context, SSID string, Password string) (string, error) {
	var devicePaths []dbus.ObjectPath
	var response string

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}

	conn, err := dbus.ConnectSystemBus()
	if err != nil {
		return "", fmt.Errorf("failed to connect to system bus: %s", err)
	}
	defer conn.Close()

	obj := conn.Object("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager")
	err = obj.Call("org.freedesktop.NetworkManager.GetDevices", 0).Store(&devicePaths)
	if err != nil {
		return "", fmt.Errorf("failed to get devices: %s", err)
	}

	// Iterate over devices to find Wi-Fi devices
	for _, devicePath := range devicePaths {
		var deviceType uint32

		device := conn.Object("org.freedesktop.NetworkManager", devicePath)
		deviceTypeVriant, err := device.GetProperty("org.freedesktop.NetworkManager.Device.DeviceType")
		if err != nil {
			return "", fmt.Errorf("failed to get device type: %s", err)
		}
		err = deviceTypeVriant.Store(&deviceType)
		if err != nil {
			return "", fmt.Errorf("failed to convert device type: %s", err)
		}

		// Check if the device is a Wi-Fi device (type 2)
		if deviceType == 2 {
			var connectionPath dbus.ObjectPath
			var activeConnectionPath dbus.ObjectPath

			settings := map[string]map[string]dbus.Variant{
				"connection": {
					"id":          dbus.MakeVariant(SSID),
					"type":        dbus.MakeVariant("802-11-wireless"),
					"autoconnect": dbus.MakeVariant(true),
				},
				"802-11-wireless": {
					"ssid":     dbus.MakeVariant([]byte(SSID)),
					"mode":     dbus.MakeVariant("infrastructure"),
					"security": dbus.MakeVariant("802-11-wireless-security"),
				},
				"802-11-wireless-security": {
					"key-mgmt": dbus.MakeVariant("wpa-psk"),
					"psk":      dbus.MakeVariant(Password),
				},
			}

			// Add a new connection and connect
			err := obj.Call("org.freedesktop.NetworkManager.AddAndActivateConnection", 0, settings, devicePath, dbus.ObjectPath("/")).Store(&connectionPath, &activeConnectionPath)
			if err != nil {
				return "", fmt.Errorf("failed to add or connect to %s: %s", SSID, err)
			}

			// Register StateChanged signal
			channel := make(chan *dbus.Signal, 1)
			conn.Signal(channel)
			err = conn.AddMatchSignal(
				dbus.WithMatchInterface("org.freedesktop.NetworkManager.Connection.Active"),
				dbus.WithMatchMember("StateChanged"),
				dbus.WithMatchObjectPath(activeConnectionPath),
			)
			if err != nil {
				return "", fmt.Errorf("failed to add match signal: %s", err)
			}

			// Check the connection activated successfully
			for signal := range channel {
				fmt.Println(signal.Name)
				if signal.Name != "org.freedesktop.NetworkManager.Connection.Active.StateChanged" {
					continue
				}
				switch state := signal.Body[0].(uint32); state {
				case NM_ACTIVE_CONNECTION_STATE_UNKNOWN:
					return "", fmt.Errorf("unexpected active connection state: unknown")
				case NM_ACTIVE_CONNECTION_STATE_ACTIVATING:
					response = "Connecting to " + SSID
				case NM_ACTIVE_CONNECTION_STATE_ACTIVATED:
					response = "Connected to " + SSID + " successfully"
					return response, nil
				case NM_ACTIVE_CONNECTION_STATE_DEACTIVATING:
					return "", fmt.Errorf("unexpected active connection state: deactivating")
				case NM_ACTIVE_CONNECTION_STATE_DEACTIVATED:
					return "", fmt.Errorf("unexpected active connection state: deactivated")
				default:
					return "", fmt.Errorf("unknown active connection state")
				}
			}
		}
	}
	return string(response), nil
}

func (c *WifiController) Disconnect(ctx context.Context) (string, error) {
	var devicePaths []dbus.ObjectPath
	response := "wifi disconnection is failed"

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}

	conn, err := dbus.ConnectSystemBus()
	if err != nil {
		return "", fmt.Errorf("failed to connect to system bus: %s", err)
	}
	defer conn.Close()

	obj := conn.Object("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager")
	err = obj.Call("org.freedesktop.NetworkManager.GetDevices", 0).Store(&devicePaths)
	if err != nil {
		return "", fmt.Errorf("failed to get devices: %s", err)
	}

	// Iterate over devices to find Wi-Fi devices
	for _, devicePath := range devicePaths {
		var deviceType uint32

		device := conn.Object("org.freedesktop.NetworkManager", devicePath)
		deviceTypeVriant, err := device.GetProperty("org.freedesktop.NetworkManager.Device.DeviceType")
		if err != nil {
			return "", fmt.Errorf("failed to get device type: %s", err)
		}
		err = deviceTypeVriant.Store(&deviceType)
		if err != nil {
			return "", fmt.Errorf("failed to convert device type: %s", err)
		}

		// Check if the device is a Wi-Fi device (type 2)
		if deviceType == 2 {
			// Add a new connection and connect
			disconnect := device.Call("org.freedesktop.NetworkManager.Device.Disconnect", 0)
			if disconnect.Err != nil {
				return "", fmt.Errorf("failed to disconnect %s: %s", devicePath, err)
			} else {
				var intf string
				interfaceVariant, _ := device.GetProperty("org.freedesktop.NetworkManager.Device.Interface")
				_ = interfaceVariant.Store(&intf)
				response = intf + " disconnected successfully"
			}
		}
	}
	return string(response), nil
}

func (c *WifiController) WifiRadioSwitch(ctx context.Context, TurnOn bool) (string, error) {
	var wifiEnabled bool
	var status string

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}

	conn, err := dbus.ConnectSystemBus()
	if err != nil {
		return "", fmt.Errorf("failed to connect to system bus: %s", err)
	}
	defer conn.Close()

	obj := conn.Object("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager")
	err = obj.SetProperty("org.freedesktop.NetworkManager.WirelessEnabled", dbus.MakeVariant(TurnOn))
	if err != nil {
		return "", fmt.Errorf("failed to set wifi status: %s", err)
	}

	// Check the wifi status
	wifiEnabledVariant, err := obj.GetProperty("org.freedesktop.NetworkManager.WirelessEnabled")
	if err != nil {
		return "", fmt.Errorf("failed to get wifi status: %s", err)
	}
	err = wifiEnabledVariant.Store(&wifiEnabled)
	if err != nil {
		return "", fmt.Errorf("failed to get wifi status: %s", err)
	}

	if TurnOn {
		status = "enabled"
	} else {
		status = "disabled"
	}
	return fmt.Sprintf("Wireless %s successfully", status), nil
}
