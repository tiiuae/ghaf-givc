// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package config

import (
	"crypto/tls"
	"encoding/json"
	"fmt"
	"os"
	"reflect"
	"strings"

	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"
)

// Capability type constants
const (
	CapabilityExec        = "Exec"
	CapabilityWifi        = "Wifi"
	CapabilityCtap        = "Ctap"
	CapabilityHwid        = "Hwid"
	CapabilityNotifier    = "Notifier"
	CapabilityEventProxy  = "EventProxy"
	CapabilitySocketProxy = "SocketProxy"
	CapabilityApps        = "Applications"
	CapabilityServices    = "Services"
	CapabilityVmManager   = "VMManager"
)

// Agent type constants
const (
	AgentTypeHost  = "host"
	AgentTypeSys   = "sys"
	AgentTypeApp   = "app"
	AgentTypeAdmin = "admin"
)

// Agent category constants
const (
	AgentCategoryService     = "service"
	AgentCategoryApplication = "application"
)

// Allowed Capabilities
var AllowedCapabilities = map[string][]string{
	AgentTypeHost: {
		CapabilityExec,
		CapabilityServices,
		CapabilityVmManager,
		CapabilityEventProxy,
	},
	AgentTypeSys: {
		CapabilityWifi,
		CapabilityCtap,
		CapabilityHwid,
		CapabilityNotifier,
		CapabilityEventProxy,
		CapabilitySocketProxy,
	},
	AgentTypeApp: {
		CapabilityEventProxy,
		CapabilitySocketProxy,
		CapabilityApps,
	},
	AgentTypeAdmin: {},
}

// JSONConfig, the root givc-agent configuration structure
type JSONConfig struct {
	Agent        AgentConfigJSON         `json:"agent"`
	AdminServer  AdminServerJSON         `json:"adminServer"`
	TLS          *TLSConfigJSON          `json:"tls,omitempty"`
	Policy       *PolicyConfigJSON       `json:"policy,omitempty"`
	Capabilities *CapabilitiesConfigJSON `json:"capabilities,omitempty"`
}

type AgentConfigJSON struct {
	Name     string `json:"name"`
	Type     string `json:"type"`
	Parent   string `json:"parent,omitempty"`
	IPAddr   string `json:"ipaddr"`
	Port     string `json:"port"`
	Protocol string `json:"protocol"`
}

type AdminServerJSON struct {
	Name     string `json:"name"`
	IPAddr   string `json:"ipaddr"`
	Port     string `json:"port"`
	Protocol string `json:"protocol"`
}

type TLSConfigJSON struct {
	Enable     bool   `json:"enable"`
	CaCertPath string `json:"caCertPath,omitempty"`
	CertPath   string `json:"certPath,omitempty"`
	KeyPath    string `json:"keyPath,omitempty"`
}

type EventConfigJSON struct {
	Name     string `json:"name"`
	IPAddr   string `json:"ipaddr"`
	Port     string `json:"port"`
	Protocol string `json:"protocol"`
	Producer bool   `json:"producer"`
	Device   string `json:"device"`
}

type SocketConfigJSON struct {
	Name     string `json:"name"`
	IPAddr   string `json:"ipaddr"`
	Port     string `json:"port"`
	Protocol string `json:"protocol"`
	Server   bool   `json:"server"`
	Socket   string `json:"socket"`
}

type PolicyConfigJSON struct {
	Enabled   bool              `json:"enable"`
	StorePath string            `json:"storePath,omitempty"`
	Policies  map[string]string `json:"policies,omitempty"`
}

type CapabilitiesConfigJSON struct {
	Services     []string                `json:"services,omitempty"`
	VMManager    *VMManagerUnitsConfig   `json:"vmManager,omitempty"`
	Applications []ApplicationConfigJSON `json:"applications,omitempty"`
	Exec         *ExecCapabilityJSON     `json:"exec,omitempty"`
	Wifi         *WifiCapabilityJSON     `json:"wifi,omitempty"`
	Ctap         *CtapCapabilityJSON     `json:"ctap,omitempty"`
	Hwid         *HwidCapabilityJSON     `json:"hwid,omitempty"`
	Notifier     *NotifierCapabilityJSON `json:"notifier,omitempty"`
	EventProxy   *EventProxyJSON         `json:"eventProxy,omitempty"`
	SocketProxy  *SocketProxyJSON        `json:"socketProxy,omitempty"`
}

type VMManagerUnitsConfig struct {
	Admvms []string `json:"admvms,omitempty"`
	Sysvms []string `json:"sysvms,omitempty"`
	Appvms []string `json:"appvms,omitempty"`
}

type ExecCapabilityJSON struct {
	Enabled bool `json:"enable"`
}

type WifiCapabilityJSON struct {
	Enabled bool `json:"enable"`
}

type CtapCapabilityJSON struct {
	Enabled bool `json:"enable"`
}

type HwidCapabilityJSON struct {
	Enabled   bool   `json:"enable"`
	Interface string `json:"interface,omitempty"`
}

type NotifierCapabilityJSON struct {
	Enabled bool   `json:"enable"`
	Socket  string `json:"socket,omitempty"`
}

type EventProxyJSON struct {
	Enabled bool              `json:"enable"`
	Events  []EventConfigJSON `json:"events,omitempty"`
}

type SocketProxyJSON struct {
	Enabled bool               `json:"enable"`
	Sockets []SocketConfigJSON `json:"sockets,omitempty"`
}

type ApplicationConfigJSON struct {
	Name        string   `json:"name"`
	Command     string   `json:"command"`
	Args        []string `json:"args,omitempty"`
	Directories []string `json:"directories,omitempty"`
}

func LoadConfig(filePath string) (*AgentConfig, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return nil, fmt.Errorf("failed to read config file: %w", err)
	}

	var jsonConfig JSONConfig
	if err := json.Unmarshal(data, &jsonConfig); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %w", err)
	}

	// Convert JSON config to AgentConfig
	config, err := getAgentConfig(&jsonConfig)
	if err != nil {
		return nil, fmt.Errorf("failed to convert config: %w", err)
	}

	return config, nil
}

func contains(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}

func validateCapabilities(jsonConfig *JSONConfig) error {
	parts := strings.Split(jsonConfig.Agent.Type, ":")
	if len(parts) != 2 {
		return fmt.Errorf("invalid agent type format: %s. Expected format: <type>:<subtype>", jsonConfig.Agent.Type)
	}

	allowedCaps, exists := AllowedCapabilities[parts[0]]

	if !exists {
		return fmt.Errorf("Invalid agent: %s", parts[0])
	}

	v := reflect.ValueOf(jsonConfig.Capabilities)
	if v.Kind() != reflect.Pointer || v.IsNil() {
		return nil
	}

	v = v.Elem()
	t := v.Type()

	for i := 0; i < v.NumField(); i++ {
		fv := v.Field(i)
		ft := t.Field(i)

		// Only pointer fields represent capabilities
		if fv.Kind() != reflect.Pointer || fv.IsNil() {
			continue
		}

		elem := fv.Elem()
		if elem.Kind() != reflect.Struct {
			continue
		}

		// Check Enable if it exists
		enableField := elem.FieldByName("Enabled")
		if enableField.IsValid() && enableField.Kind() == reflect.Bool {
			if !enableField.Bool() {
				continue
			}
		}
		if !contains(allowedCaps, ft.Name) {
			return fmt.Errorf("capability [%s] not allowed for agent type %s", ft.Name, jsonConfig.Agent.Type)
		}
	}

	return nil
}

// JSONConfig to AgentConfig
func getAgentConfig(jsonConfig *JSONConfig) (*AgentConfig, error) {
	config := &AgentConfig{}

	if err := validateCapabilities(jsonConfig); err != nil {
		return nil, fmt.Errorf("capability validation failed: %w", err)
	}

	if err := getIdentityConfig(&config.Identity, &jsonConfig.Agent); err != nil {
		return nil, fmt.Errorf("failed to convert identity config: %w", err)
	}

	if jsonConfig.Policy != nil {
		getPolicyConfig(&config.Policy, jsonConfig.Policy)
	} else {
		config.Policy = PolicyConfig{
			PolicyAdminEnabled: false,
			PolicyStorePath:    "",
			PoliciesJson:       make(map[string]string),
		}
	}

	if jsonConfig.Capabilities != nil {
		if err := getCapabilitiesConfig(&config.Capabilities, jsonConfig.Capabilities, config.Identity.SubType); err != nil {
			return nil, fmt.Errorf("failed to convert capabilities config: %w", err)
		}
	} else {
		config.Capabilities = CapabilitiesConfig{
			Units:        make(map[string]uint32),
			Applications: make([]givc_types.ApplicationManifest, 0),
		}
	}

	if err := getNetworkConfig(&config.Network, jsonConfig, &config.Identity, &config.Capabilities); err != nil {
		return nil, fmt.Errorf("failed to convert network config: %w", err)
	}

	return config, nil
}

// Identity configuration from JSON
func getIdentityConfig(identity *IdentityConfig, agentJSON *AgentConfigJSON) error {
	identity.Name = agentJSON.Name
	identity.ServiceName = "givc-" + agentJSON.Name + ".service"
	identity.Parent = agentJSON.Parent

	var err error
	identity.Type, identity.SubType, err = GetAgentType(agentJSON.Type)
	if err != nil {
		return err
	}

	return nil
}

// Policy configuration from JSON
func getPolicyConfig(policy *PolicyConfig, jsonPolicy *PolicyConfigJSON) {
	policy.PolicyAdminEnabled = jsonPolicy.Enabled
	policy.PolicyStorePath = jsonPolicy.StorePath
	if jsonPolicy.Policies != nil {
		policy.PoliciesJson = jsonPolicy.Policies
	} else {
		policy.PoliciesJson = make(map[string]string)
	}
}

// Network configuration from JSON
func getNetworkConfig(network *NetworkConfig, jsonConfig *JSONConfig, identity *IdentityConfig, capabilities *CapabilitiesConfig) error {
	// Parse TLS configuration
	var tlsConfig *tls.Config
	var err error
	if jsonConfig.TLS != nil {
		tlsConfig, err = getTLSConfig(jsonConfig.TLS)
		if err != nil {
			return fmt.Errorf("failed to convert TLS config: %w", err)
		}
	}
	network.TlsConfig = tlsConfig

	// Create admin endpoint
	network.AdminEndpoint = &givc_types.EndpointConfig{
		Transport: givc_types.TransportConfig{
			Name:     jsonConfig.AdminServer.Name,
			Address:  jsonConfig.AdminServer.IPAddr,
			Port:     jsonConfig.AdminServer.Port,
			Protocol: jsonConfig.AdminServer.Protocol,
		},
		TlsConfig: tlsConfig,
		Services:  nil,
	}

	// Create agent endpoint config with services
	var services []string
	services = append(services, identity.ServiceName)
	for unit := range capabilities.Units {
		services = append(services, unit)
	}

	network.AgentEndpoint = &givc_types.EndpointConfig{
		Transport: givc_types.TransportConfig{
			Name:     jsonConfig.Agent.Name,
			Address:  jsonConfig.Agent.IPAddr,
			Port:     jsonConfig.Agent.Port,
			Protocol: jsonConfig.Agent.Protocol,
		},
		TlsConfig: tlsConfig,
		Services:  services,
	}

	// Convert bridge configuration from capabilities proxies
	if jsonConfig.Capabilities != nil {
		if jsonConfig.Capabilities.EventProxy != nil && jsonConfig.Capabilities.EventProxy.Enabled {
			network.Bridge.Events = make([]givc_types.EventConfig, len(jsonConfig.Capabilities.EventProxy.Events))
			for i, event := range jsonConfig.Capabilities.EventProxy.Events {
				network.Bridge.Events[i] = givc_types.EventConfig{
					Transport: givc_types.TransportConfig{
						Name:     event.Name,
						Address:  event.IPAddr,
						Port:     event.Port,
						Protocol: event.Protocol,
					},
					Producer:  event.Producer,
					Device:    event.Device,
					TlsConfig: tlsConfig,
				}
			}
		} else {
			network.Bridge.Events = make([]givc_types.EventConfig, 0)
		}

		if jsonConfig.Capabilities.SocketProxy != nil && jsonConfig.Capabilities.SocketProxy.Enabled {
			network.Bridge.Sockets = make([]givc_types.ProxyConfig, len(jsonConfig.Capabilities.SocketProxy.Sockets))
			for i, socket := range jsonConfig.Capabilities.SocketProxy.Sockets {
				network.Bridge.Sockets[i] = givc_types.ProxyConfig{
					Transport: givc_types.TransportConfig{
						Name:     socket.Name,
						Address:  socket.IPAddr,
						Port:     socket.Port,
						Protocol: socket.Protocol,
					},
					Server:    socket.Server,
					Socket:    socket.Socket,
					TlsConfig: tlsConfig,
				}
			}
		} else {
			network.Bridge.Sockets = make([]givc_types.ProxyConfig, 0)
		}
	} else {
		network.Bridge.Events = make([]givc_types.EventConfig, 0)
		network.Bridge.Sockets = make([]givc_types.ProxyConfig, 0)
	}

	return nil
}

func GetAgentType(input string) (uint32, uint32, error) {
	parts := strings.Split(input, ":")
	if len(parts) != 2 {
		return 0, 0, fmt.Errorf("invalid agent type format: %s. Expected format: <type>:<subtype>", input)
	}

	agentType := parts[0]
	agentSubType := parts[1]

	if agentSubType != AgentCategoryService && agentSubType != AgentCategoryApplication {
		return 0, 0, fmt.Errorf("invalid subtype: %s", agentSubType)
	}

	switch agentType {
	case AgentTypeHost:
		if agentSubType == AgentCategoryService {
			return givc_types.UNIT_TYPE_HOST_MGR, givc_types.UNIT_TYPE_HOST_SVC, nil
		}
		return givc_types.UNIT_TYPE_HOST_MGR, givc_types.UNIT_TYPE_HOST_APP, nil

	case AgentTypeSys:
		if agentSubType == AgentCategoryService {
			return givc_types.UNIT_TYPE_SYSVM_MGR, givc_types.UNIT_TYPE_SYSVM_SVC, nil
		}
		return givc_types.UNIT_TYPE_SYSVM_MGR, givc_types.UNIT_TYPE_SYSVM_APP, nil

	case AgentTypeAdmin:
		if agentSubType == AgentCategoryService {
			return givc_types.UNIT_TYPE_ADMVM_MGR, givc_types.UNIT_TYPE_ADMVM_SVC, nil
		}
		return givc_types.UNIT_TYPE_ADMVM_MGR, givc_types.UNIT_TYPE_ADMVM_APP, nil

	case AgentTypeApp:
		if agentSubType == AgentCategoryService {
			return givc_types.UNIT_TYPE_APPVM_MGR, givc_types.UNIT_TYPE_APPVM_SVC, nil
		}
		return givc_types.UNIT_TYPE_APPVM_MGR, givc_types.UNIT_TYPE_APPVM_APP, nil

	default:
		return 0, 0, fmt.Errorf("invalid agent type: %s", agentType)
	}
}

// TLS configuration from JSON
func getTLSConfig(jsonTLS *TLSConfigJSON) (*tls.Config, error) {
	if jsonTLS.Enable {
		tlsConfig, err := givc_util.TlsServerConfig(jsonTLS.CaCertPath, jsonTLS.CertPath, jsonTLS.KeyPath, true)
		if err != nil {
			return nil, fmt.Errorf("failed to create TLS config: %w", err)
		}
		return tlsConfig, nil
	}

	return nil, nil
}

// Capabilities configuration from JSON
func getCapabilitiesConfig(capabilities *CapabilitiesConfig, jsonCapabilities *CapabilitiesConfigJSON, agentSubType uint32) error {
	capabilities.Units = make(map[string]uint32)

	if jsonCapabilities.Services != nil {
		for _, service := range jsonCapabilities.Services {
			capabilities.Units[service] = agentSubType
		}
	}

	if jsonCapabilities.VMManager != nil {
		if jsonCapabilities.VMManager.Admvms != nil {
			for _, vm := range jsonCapabilities.VMManager.Admvms {
				capabilities.Units[vm] = givc_types.UNIT_TYPE_ADMVM
			}
		}

		if jsonCapabilities.VMManager.Sysvms != nil {
			for _, vm := range jsonCapabilities.VMManager.Sysvms {
				capabilities.Units[vm] = givc_types.UNIT_TYPE_SYSVM
			}
		}

		if jsonCapabilities.VMManager.Appvms != nil {
			for _, vm := range jsonCapabilities.VMManager.Appvms {
				capabilities.Units[vm] = givc_types.UNIT_TYPE_APPVM
			}
		}
	}

	if jsonCapabilities.Applications != nil {
		capabilities.Applications = make([]givc_types.ApplicationManifest, len(jsonCapabilities.Applications))
		for i, app := range jsonCapabilities.Applications {
			args := app.Args
			if args == nil {
				args = make([]string, 0)
			}
			directories := app.Directories
			if directories == nil {
				directories = make([]string, 0)
			}

			capabilities.Applications[i] = givc_types.ApplicationManifest{
				Name:        app.Name,
				Command:     app.Command,
				Args:        args,
				Directories: directories,
			}
		}
	} else {
		capabilities.Applications = make([]givc_types.ApplicationManifest, 0)
	}

	if jsonCapabilities.Exec != nil {
		capabilities.Exec = ExecCapability{
			Enabled: jsonCapabilities.Exec.Enabled,
		}
	}

	if jsonCapabilities.Wifi != nil {
		capabilities.Wifi = WifiCapability{
			Enabled: jsonCapabilities.Wifi.Enabled,
		}
	}

	if jsonCapabilities.Ctap != nil {
		capabilities.Ctap = CtapCapability{
			Enabled: jsonCapabilities.Ctap.Enabled,
		}
	}

	if jsonCapabilities.Hwid != nil {
		capabilities.Hwid = HwidCapability{
			Enabled:   jsonCapabilities.Hwid.Enabled,
			Interface: jsonCapabilities.Hwid.Interface,
		}
	}

	if jsonCapabilities.Notifier != nil {
		capabilities.Notifier = NotifierCapability{
			Enabled: jsonCapabilities.Notifier.Enabled,
			Socket:  jsonCapabilities.Notifier.Socket,
		}
	}

	if jsonCapabilities.EventProxy != nil {
		capabilities.EventProxy = EventProxyCapability{
			Enabled: jsonCapabilities.EventProxy.Enabled,
		}
	}

	if jsonCapabilities.SocketProxy != nil {
		capabilities.SocketProxy = SocketProxyCapability{
			Enabled: jsonCapabilities.SocketProxy.Enabled,
		}
		// Sockets are used in network config
	}

	return nil
}
