// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package wifimanager

import (
	"context"
	"encoding/json"
	"fmt"

	givc_wifi "givc/modules/api/wifi"

	"github.com/godbus/dbus/v5"
	log "github.com/sirupsen/logrus"
)

type WifiController struct {
	conn            *dbus.Conn
	wifiDevices     []dbus.BusObject
	wifiDevicePaths []dbus.ObjectPath
	nm              dbus.BusObject
}

type WifiNetworkResponse struct {
	Ssid     string
	Signal   uint32
	Security string
}

type AP struct {
	SSID        string
	Strength    uint32
	HWAdress    string
	PrivacyFlag bool
	WPAFlags    uint32
	RSNFlags    uint32
	Security    string
}

const (
	NmAPSecConNone       = "none"
	NmAPSecConDynamicWEP = "ieee8021x"
	NmAPSecConOWE        = "owe"
	NmAPSecConWPAPSK     = "wpa-psk"
	NmAPSecConSAE        = "sae"
	NmAPSecConWPAEAP     = "wpa-eap"
)

const (
	NmActiveConnectionStateUnknown uint32 = iota
	NmActiveConnectionStateActivating
	NmActiveConnectionStateActivated
	NmActiveConnectionStateDeactivating
	NmActiveConnectionStateDeactivated
)

const (
	NmDeviceTypeUnknown  uint32 = iota // unknown device
	NmDeviceTypeEthernet               // a wired ethernet device
	NmDeviceTypeWifi                   // an 802.11 Wi-Fi device
)

const (
	Nm80211APSecNone                uint32 = 0x0
	Nm80211APSecPairWEP40           uint32 = 0x1
	Nm80211APSecPairWEP104          uint32 = 0x2
	Nm80211APSecPairTKIP            uint32 = 0x4
	Nm80211APSecPairCCMP            uint32 = 0x8
	Nm80211APSecGroupWEP40          uint32 = 0x10
	Nm80211APSecGroupWEP104         uint32 = 0x20
	Nm80211APSecGroupTKIP           uint32 = 0x40
	Nm80211APSecGroupCCMP           uint32 = 0x80
	Nm80211APSecKeyMgmtPSK          uint32 = 0x100
	Nm80211APSecKeyMgmt8021X        uint32 = 0x200
	Nm80211APSecKeyMgmtSAE          uint32 = 0x400
	Nm80211APSecKeyMgmtOWE          uint32 = 0x800
	Nm80211APSecKeyMgmtOWETM        uint32 = 0x1000
	Nm80211APSecKeyMgmtEAPSuiteB192 uint32 = 0x2000
)

func NewController() (*WifiController, error) {
	var err error
	var c WifiController

	c.conn, err = dbus.ConnectSystemBus()
	if err != nil {
		return nil, fmt.Errorf("failed to connect to system bus: %s", err)
	}

	c.nm = c.conn.Object("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager")

	c.wifiDevices, err = c.GetWifiDevices()
	if err != nil {
		return nil, fmt.Errorf("failed to get wifi devices: %s", err)
	}

	if err = c.conn.AddMatchSignal(
		dbus.WithMatchObjectPath("/org/freedesktop/NetworkManager"),
		dbus.WithMatchInterface("org.freedesktop.NetworkManager"),
		dbus.WithMatchSender("org.freedesktop.NetworkManager"),
	); err != nil {
		return nil, fmt.Errorf("failed to set device signal handler: %s", err)
	}

	// Update the wifi device when it is enabled lately
	channel := make(chan *dbus.Signal, 10)
	c.conn.Signal(channel)
	go c.signal_handle(channel)

	return &c, nil
}

func (c *WifiController) Close() {
	err := c.conn.Close()
	if err != nil {
		log.Warnf("[WifiController] failed to close connection: %s", err)
	}
}

func (c *WifiController) GetNetworkList(ctx context.Context, NetworkInterface string) ([]*givc_wifi.AccessPoint, error) {
	var output []*givc_wifi.AccessPoint

	// Input validation
	if ctx == nil {
		return output, fmt.Errorf("context cannot be nil")
	}

	// Iterate over Wi-Fi devices
	for _, device := range c.wifiDevices {
		var apPaths []dbus.ObjectPath

		err := device.Call("org.freedesktop.NetworkManager.Device.Wireless.GetAllAccessPoints", 0).Store(&apPaths)
		if err != nil {
			return output, fmt.Errorf("failed to get access points: %s", err)
		}
		// Iterate over access points and append into output
		for _, apPath := range apPaths {
			var accesspoint AP

			ap := c.conn.Object("org.freedesktop.NetworkManager", apPath)
			accesspoint, err := GetAPData(ap)
			if err != nil {
				return output, fmt.Errorf("failed to get accesspoint data: %s", err)
			}
			accesspoint.Security = GetAPSecurity(accesspoint)

			// Append variables into output
			network := givc_wifi.AccessPoint{
				SSID:     accesspoint.SSID,
				Signal:   accesspoint.Strength,
				Security: accesspoint.Security,
			}
			output = append(output, &network)
		}
	}
	return output, nil
}

func (c *WifiController) GetActiveConnection(ctx context.Context) (bool, string, uint32, string, error) {
	var accesspoint AP

	// Input validation
	if ctx == nil {
		return false, "", 0, "", fmt.Errorf("context cannot be nil")
	}

	// Iterate over Wi-Fi devices
	for _, device := range c.wifiDevices {
		var ap string
		activeAPPath, err := device.GetProperty("org.freedesktop.NetworkManager.Device.Wireless.ActiveAccessPoint")
		if err != nil {
			return false, "", 0, "", fmt.Errorf("failed to get active access point path: %s", err)
		}
		err = activeAPPath.Store(&ap)
		if err != nil {
			return false, "", 0, "", fmt.Errorf("failed to store active access point path: %s", err)
		}
		// No active connection
		if ap == "/" {
			return false, "", 0, "", nil
		}

		activeAP := c.conn.Object("org.freedesktop.NetworkManager", dbus.ObjectPath(ap))
		accesspoint, err = GetAPData(activeAP)
		if err != nil {
			return true, "", 0, "", fmt.Errorf("failed to get access point data: %s", err)
		}
		accesspoint.Security = GetAPSecurity(accesspoint)
	}
	return true, accesspoint.SSID, accesspoint.Strength, accesspoint.Security, nil
}

func (c *WifiController) Connect(ctx context.Context, SSID string, Password string, extendSettings string) (string, error) {
	var response string

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}

	// Iterate over Wi-Fi devices
	for _, device := range c.wifiDevices {
		var connectionPath dbus.ObjectPath
		var activeConnectionPath dbus.ObjectPath
		var keymgmt string

		var apPaths []dbus.ObjectPath
		err := device.Call("org.freedesktop.NetworkManager.Device.Wireless.GetAllAccessPoints", 0).Store(&apPaths)
		if err != nil {
			return "", fmt.Errorf("failed to get access points: %s", err)
		}
		if len(apPaths) < 1 {
			continue
		}

		// Iterate over access points and append into output
		for _, apPath := range apPaths {
			apObject := c.conn.Object("org.freedesktop.NetworkManager", apPath)
			accesspoint, err := GetAPData(apObject)
			if err != nil {
				return "", fmt.Errorf("failed to get access point data: %s", err)
			}

			if accesspoint.SSID != SSID {
				continue
			}

			// AP security flags match by @forxn9 https://github.com/NetworkManager/NetworkManager/commit/31a12ee344432b28d9d04c9ed2bfe8596790228a

			if (accesspoint.WPAFlags&Nm80211APSecKeyMgmt8021X) > 0 || (accesspoint.RSNFlags&Nm80211APSecKeyMgmt8021X) > 0 {
				keymgmt = NmAPSecConWPAEAP
			} else if (accesspoint.RSNFlags&Nm80211APSecKeyMgmtOWE) > 0 || (accesspoint.RSNFlags&Nm80211APSecKeyMgmtOWETM) > 0 {
				keymgmt = NmAPSecConOWE
			} else if accesspoint.RSNFlags&Nm80211APSecKeyMgmtSAE > 0 {
				keymgmt = NmAPSecConSAE
			} else if (accesspoint.RSNFlags&Nm80211APSecKeyMgmtPSK) > 0 || (accesspoint.RSNFlags&Nm80211APSecKeyMgmt8021X) > 0 {
				keymgmt = NmAPSecConWPAPSK
			} else if accesspoint.PrivacyFlag && (accesspoint.WPAFlags == Nm80211APSecNone) && (accesspoint.RSNFlags == Nm80211APSecNone) {
				keymgmt = NmAPSecConDynamicWEP
			} else if !accesspoint.PrivacyFlag {
				keymgmt = NmAPSecConNone
			}
		}

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
				"key-mgmt": dbus.MakeVariant(keymgmt),
				"psk":      dbus.MakeVariant(Password),
			},
		}

		if keymgmt == NmAPSecConWPAEAP {
			settings, err = MergeSettings(settings, extendSettings)
			if err != nil {
				return "", fmt.Errorf("failed to merge settings %s: %s", extendSettings, err)
			}
		}

		// Add a new connection and connect
		err = c.nm.Call("org.freedesktop.NetworkManager.AddAndActivateConnection", 0, settings, device.Path(), dbus.ObjectPath("/")).Store(&connectionPath, &activeConnectionPath)
		if err != nil {
			return "", fmt.Errorf("failed to add or connect to %s: %s", SSID, err)
		}

		// Register StateChanged signal
		channel := make(chan *dbus.Signal, 1)
		c.conn.Signal(channel)
		err = c.conn.AddMatchSignal(
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
			case NmActiveConnectionStateUnknown:
				return "", fmt.Errorf("unexpected active connection state: unknown")
			case NmActiveConnectionStateActivating:
				response = "Connecting to " + SSID
			case NmActiveConnectionStateActivated:
				response = "Connected to " + SSID + " successfully"
				return response, nil
			case NmActiveConnectionStateDeactivating:
				return "", fmt.Errorf("unexpected active connection state: deactivating")
			case NmActiveConnectionStateDeactivated:
				return "", fmt.Errorf("unexpected active connection state: deactivated")
			default:
				return "", fmt.Errorf("unknown active connection state")
			}
		}
	}
	return string(response), nil
}

func (c *WifiController) Disconnect(ctx context.Context) (string, error) {
	response := "wifi disconnection is failed"

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}

	// Iterate over Wi-Fi devices
	for _, device := range c.wifiDevices {
		disconnect := device.Call("org.freedesktop.NetworkManager.Device.Disconnect", 0)
		if disconnect.Err != nil {
			return "", fmt.Errorf("failed to disconnect %s: %s", device.Path(), disconnect.Err)
		} else {
			var intf string
			interfaceVariant, _ := device.GetProperty("org.freedesktop.NetworkManager.Device.Interface")
			_ = interfaceVariant.Store(&intf)
			response = intf + " disconnected successfully"
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

	// Set the wifi status
	err := c.nm.SetProperty("org.freedesktop.NetworkManager.WirelessEnabled", dbus.MakeVariant(TurnOn))
	if err != nil {
		return "", fmt.Errorf("failed to set wifi status: %s", err)
	}

	// Check the wifi status
	wifiEnabledVariant, err := c.nm.GetProperty("org.freedesktop.NetworkManager.WirelessEnabled")
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

func (c *WifiController) GetWifiDevices() ([]dbus.BusObject, error) {
	var wifiDevices []dbus.BusObject
	var deviceType uint32
	var devicePaths []dbus.ObjectPath

	err := c.nm.Call("org.freedesktop.NetworkManager.GetDevices", 0).Store(&devicePaths)
	if err != nil {
		return nil, fmt.Errorf("failed to get devices: %s", err)
	}

	c.wifiDevicePaths = devicePaths

	for _, devicePath := range devicePaths {
		device := c.conn.Object("org.freedesktop.NetworkManager", devicePath)
		deviceManaged, err := device.GetProperty("org.freedesktop.NetworkManager.Device.Managed")
		if err != nil {
			return nil, fmt.Errorf("failed to get device type: %s", err)
		}
		if !deviceManaged.Value().(bool) {
			continue // skip unmanaged devices
		}
		deviceTypeVriant, err := device.GetProperty("org.freedesktop.NetworkManager.Device.DeviceType")
		if err != nil {
			return nil, fmt.Errorf("failed to get device type: %s", err)
		}
		err = deviceTypeVriant.Store(&deviceType)
		if err != nil {
			return nil, fmt.Errorf("failed to convert device type: %s", err)
		}

		// Check if the device is a Wi-Fi device
		if deviceType == NmDeviceTypeWifi {
			wifiDevices = append(wifiDevices, device)
		}
	}
	return wifiDevices, nil
}

func (c *WifiController) signal_handle(channel chan *dbus.Signal) {
	var err error

	for signal := range channel {
		if signal.Name == "org.freedesktop.NetworkManager.DeviceAdded" {
			c.wifiDevices, err = c.GetWifiDevices()
			if err != nil {
				log.Warnf("[WifiController] failed to get wifi devices: %s", err)
				continue
			}
			log.Infof("[WifiController] wifi devices updated!")
		}
	}
}

func GetAPSecurity(ap AP) string {
	if len(ap.Security) > 0 {
		return ap.Security
	}
	if ap.PrivacyFlag && (ap.WPAFlags == Nm80211APSecNone) && (ap.RSNFlags == Nm80211APSecNone) {
		ap.Security += "WEP "
	}
	if ap.WPAFlags != Nm80211APSecNone {
		ap.Security += "WPA "
	}
	if (ap.RSNFlags&Nm80211APSecKeyMgmtPSK) > 0 || (ap.RSNFlags&Nm80211APSecKeyMgmt8021X) > 0 {
		ap.Security += "WPA2 "
	}
	if ap.RSNFlags&Nm80211APSecKeyMgmtSAE > 0 {
		ap.Security += "WPA3 "
	}
	if (ap.RSNFlags&Nm80211APSecKeyMgmtOWE) > 0 || (ap.RSNFlags&Nm80211APSecKeyMgmtOWETM) > 0 {
		ap.Security += "OWE "
	}
	if (ap.WPAFlags&Nm80211APSecKeyMgmt8021X) > 0 || (ap.RSNFlags&Nm80211APSecKeyMgmt8021X) > 0 {
		ap.Security += "802.1X "
	}
	if ap.PrivacyFlag && (ap.WPAFlags != Nm80211APSecNone) && (ap.RSNFlags != Nm80211APSecNone) {
		ap.Security = "None"
	}

	return ap.Security
}

func GetAPData(ap dbus.BusObject) (AP, error) {
	var ssid []byte
	var PrivacyFlag uint32
	var accesspoint AP

	ssid_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.Ssid")
	if err != nil {
		return accesspoint, fmt.Errorf("failed to get SSID: %s", err)
	}
	err = ssid_variant.Store(&ssid)
	if err != nil {
		return accesspoint, fmt.Errorf("failed to store SSID: %s", err)
	}
	accesspoint.SSID = string(ssid)

	strength_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.Strength")
	if err != nil {
		return accesspoint, fmt.Errorf("failed to get Strength: %s", err)
	}
	err = strength_variant.Store(&(accesspoint.Strength))
	if err != nil {
		return accesspoint, fmt.Errorf("failed to store Strength: %s", err)
	}

	flags_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.Flags")
	if err != nil {
		return accesspoint, fmt.Errorf("failed to get WPA flags: %s", err)
	}
	err = flags_variant.Store(&PrivacyFlag)
	if err != nil {
		return accesspoint, fmt.Errorf("failed to store flags: %s", err)
	}
	accesspoint.PrivacyFlag = PrivacyFlag != 0

	wpaFlags_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.WpaFlags")
	if err != nil {
		return accesspoint, fmt.Errorf("failed to get WPA flags: %s", err)
	}
	err = wpaFlags_variant.Store(&(accesspoint.WPAFlags))
	if err != nil {
		return accesspoint, fmt.Errorf("failed to store WPAFlags: %s", err)
	}

	rsnFlags_variant, err := ap.GetProperty("org.freedesktop.NetworkManager.AccessPoint.RsnFlags")
	if err != nil {
		return accesspoint, fmt.Errorf("failed to get RSN flags: %s", err)
	}
	err = rsnFlags_variant.Store(&(accesspoint.RSNFlags))
	if err != nil {
		return accesspoint, fmt.Errorf("failed to store RSNFlags: %s", err)
	}

	return accesspoint, nil
}

func MergeSettings(baseSettings map[string]map[string]dbus.Variant, rawExtensionSettings string) (map[string]map[string]dbus.Variant, error) {
	var settings map[string]any

	// Parse the raw settings extension string
	err := json.Unmarshal([]byte(rawExtensionSettings), &settings)
	if err != nil {
		log.Warnf("[WifiController] failed to parse extension settings: %s", err)
		return nil, err
	}

	// Merge the two settings maps
	for setting, keys := range settings {
		// Create a new map if there is no any key
		if baseSettings[setting] == nil {
			baseSettings[setting] = make(map[string]dbus.Variant)
		}

		keymap := keys.(map[string]any)
		for key, value := range keymap {
			baseSettings[setting][key] = dbus.MakeVariant(value)
		}
	}
	return baseSettings, nil
}
