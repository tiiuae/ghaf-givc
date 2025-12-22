// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Configuration parsing and validation for the GIVC agent.
package config

import (
	"crypto/tls"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"

	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"
)

// Environment variable constants
const (
	EnvAgent          = "AGENT"
	EnvType           = "TYPE"
	EnvSubtype        = "SUBTYPE"
	EnvParent         = "PARENT"
	EnvAdminServer    = "ADMIN_SERVER"
	EnvTlsConfig      = "TLS_CONFIG"
	EnvServices       = "SERVICES"
	EnvAdmvms         = "ADMVMS"
	EnvSysvms         = "SYSVMS"
	EnvAppvms         = "APPVMS"
	EnvApplications   = "APPLICATIONS"
	EnvEventProxy     = "EVENT_PROXY"
	EnvSocketProxy    = "SOCKET_PROXY"
	EnvDebug          = "DEBUG"
	EnvExec           = "EXEC"
	EnvWifi           = "WIFI"
	EnvHwid           = "HWID"
	EnvHwidIface      = "HWID_IFACE"
	EnvNotifier       = "NOTIFIER"
	EnvNotifierSocket = "NOTIFIER_SOCKET_DIR"
	EnvPolicyAgent    = "POLICY_AGENT"
)

// AgentConfig holds the complete configuration for the GIVC agent
// Restructured with domain-based organization for better maintainability
type AgentConfig struct {
	Identity     IdentityConfig
	Network      NetworkConfig
	Capabilities CapabilitiesConfig
	Runtime      RuntimeConfig
}

// IdentityConfig
type IdentityConfig struct {
	Type        uint32 // Agent type (using existing UNIT_TYPE_* constants)
	SubType     uint32 // Agent subtype
	Parent      string // Parent agent name
	Name        string // Agent name (derived from transport config)
	ServiceName string // Systemd service name for this agent
}

// NetworkConfig
type NetworkConfig struct {
	AdminEndpoint *givc_types.EndpointConfig // Admin server endpoint (complete, ready to use)
	AgentEndpoint *givc_types.EndpointConfig // Agent endpoint (transport + TLS, services added later)
	TlsConfig     *tls.Config                // TLS configuration (for bridge configs)
	Bridge        BridgeConfig               // Inter-VM bridging services
}

// CapabilitiesConfig
type CapabilitiesConfig struct {
	Units        map[string]uint32                // Systemd units this agent can manage
	Applications []givc_types.ApplicationManifest // Applications this agent can run
	Optional     OptionalCapabilities             // Optional service capabilities
}

// RuntimeConfig
type RuntimeConfig struct {
	Debug bool // Debug mode flag
}

// BridgeConfig - Inter-VM bridging/proxy services
type BridgeConfig struct {
	Events  []givc_types.EventConfig // Event streaming bridges
	Sockets []givc_types.ProxyConfig // Socket proxy bridges
}

// OptionalCapabilities - Feature flags for optional services
type OptionalCapabilities struct {
	ExecEnabled        bool   // Remote execution capability
	WifiEnabled        bool   // WiFi management capability
	HwidEnabled        bool   // Hardware ID capability
	HwidInterface      string // Hardware interface for HWID
	NotifierEnabled    bool   // Notification service capability
	NotifierSocket     string // Socket directory for notifications
	PolicyAgentEnabled bool   // Policy agent capability
}

// parseJSONEnv parses a JSON environment variable into a target struct
func parseJSONEnv(envVar string, target any, required bool) error {
	jsonString, present := os.LookupEnv(envVar)

	if !present || jsonString == "" {
		if required {
			return fmt.Errorf("no '%s' environment variable present", envVar)
		}
		return nil
	}

	err := json.Unmarshal([]byte(jsonString), target)
	if err != nil {
		return fmt.Errorf("error parsing %s JSON: %w", envVar, err)
	}

	return nil
}

// parseAgentType parses and validates an agent type from environment variable
func parseAgentType(envVar string) (uint32, error) {
	parsedType, err := strconv.ParseUint(os.Getenv(envVar), 10, 32)
	if err != nil || parsedType > givc_types.UNIT_TYPE_APPVM_APP {
		return 0, fmt.Errorf("no or wrong '%s' environment variable present", envVar)
	}
	return uint32(parsedType), nil
}

// parseUnits parses systemd units from environment variables
func parseUnits(agentSubType uint32) map[string]uint32 {
	units := make(map[string]uint32)

	unitTypes := []struct {
		envVar   string
		unitType uint32
	}{
		{EnvServices, agentSubType},
		{EnvAdmvms, givc_types.UNIT_TYPE_ADMVM},
		{EnvSysvms, givc_types.UNIT_TYPE_SYSVM},
		{EnvAppvms, givc_types.UNIT_TYPE_APPVM},
	}

	for _, unitType := range unitTypes {
		unitsString := os.Getenv(unitType.envVar)
		if unitsString != "" {
			for unit := range strings.FieldsSeq(unitsString) {
				units[unit] = unitType.unitType
			}
		}
	}

	return units
}

// parseTLSConfig parses TLS configuration from environment variables
func parseTLSConfig() (*tls.Config, error) {
	var tlsConfigJson givc_types.TlsConfigJson
	err := parseJSONEnv(EnvTlsConfig, &tlsConfigJson, false)
	if err != nil {
		return nil, fmt.Errorf("failed to parse %s: %w", EnvTlsConfig, err)
	}

	if tlsConfigJson.Enable {
		tlsConfig, err := givc_util.TlsServerConfig(tlsConfigJson.CaCertPath, tlsConfigJson.CertPath, tlsConfigJson.KeyPath, true)
		if err != nil {
			return nil, fmt.Errorf("failed to create TLS config: %w", err)
		}
		return tlsConfig, nil
	}

	return nil, nil
}

// ParseConfig parses and validates the complete agent configuration from environment variables
func ParseConfig() (*AgentConfig, error) {
	config := &AgentConfig{}

	// Parse identity configuration
	if err := parseIdentityConfig(&config.Identity); err != nil {
		return nil, fmt.Errorf("failed to parse identity config: %w", err)
	}

	// Parse capabilities configuration
	if err := parseCapabilitiesConfig(&config.Capabilities, config.Identity.SubType); err != nil {
		return nil, fmt.Errorf("failed to parse capabilities config: %w", err)
	}

	// Parse network configuration (needs identity and capabilities for endpoint services)
	if err := parseNetworkConfig(&config.Network, &config.Identity, &config.Capabilities); err != nil {
		return nil, fmt.Errorf("failed to parse network config: %w", err)
	}

	// Parse runtime configuration
	parseRuntimeConfig(&config.Runtime)

	return config, nil
}

// parseIdentityConfig parses agent identity information
func parseIdentityConfig(identity *IdentityConfig) error {
	// Parse agent transport to get name
	var agentTransport givc_types.TransportConfig
	if err := parseJSONEnv(EnvAgent, &agentTransport, true); err != nil {
		return fmt.Errorf("failed to parse %s transport: %w", EnvAgent, err)
	}
	identity.Name = agentTransport.Name
	identity.ServiceName = "givc-" + agentTransport.Name + ".service"

	// Parse agent type
	agentType, err := parseAgentType(EnvType)
	if err != nil {
		return fmt.Errorf("failed to parse agent type: %w", err)
	}
	identity.Type = agentType

	// Parse agent subtype
	agentSubType, err := parseAgentType(EnvSubtype)
	if err != nil {
		return fmt.Errorf("failed to parse agent subtype: %w", err)
	}
	identity.SubType = agentSubType

	// Parse parent (optional)
	identity.Parent = os.Getenv(EnvParent)

	return nil
}

// parseNetworkConfig parses network and communication configuration
func parseNetworkConfig(network *NetworkConfig, identity *IdentityConfig, capabilities *CapabilitiesConfig) error {
	// Parse TLS configuration first
	tlsConfig, err := parseTLSConfig()
	if err != nil {
		return fmt.Errorf("failed to parse TLS config: %w", err)
	}
	network.TlsConfig = tlsConfig

	// Parse agent transport and create agent endpoint
	var agentTransport givc_types.TransportConfig
	if err := parseJSONEnv(EnvAgent, &agentTransport, true); err != nil {
		return fmt.Errorf("failed to parse %s transport: %w", EnvAgent, err)
	}

	// Parse admin server transport and create admin endpoint
	var adminTransport givc_types.TransportConfig
	if err := parseJSONEnv(EnvAdminServer, &adminTransport, true); err != nil {
		return fmt.Errorf("failed to parse %s transport: %w", EnvAdminServer, err)
	}

	network.AdminEndpoint = &givc_types.EndpointConfig{
		Transport: adminTransport,
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
		Transport: agentTransport,
		TlsConfig: tlsConfig,
		Services:  services,
	}

	// Parse bridge configuration
	if err := parseBridgeConfig(&network.Bridge); err != nil {
		return fmt.Errorf("failed to parse bridge config: %w", err)
	}

	return nil
}

// parseCapabilitiesConfig parses what the agent can do
func parseCapabilitiesConfig(capabilities *CapabilitiesConfig, agentSubType uint32) error {
	// Parse systemd units
	capabilities.Units = parseUnits(agentSubType)

	// Parse applications
	if err := parseJSONEnv(EnvApplications, &capabilities.Applications, false); err != nil {
		return fmt.Errorf("failed to parse applications: %w", err)
	}

	// Parse optional capabilities
	parseOptionalCapabilities(&capabilities.Optional)

	return nil
}

// parseRuntimeConfig parses runtime behavior configuration
func parseRuntimeConfig(runtime *RuntimeConfig) {
	runtime.Debug = os.Getenv(EnvDebug) == "true"
}

// parseBridgeConfig parses inter-VM bridge configuration
func parseBridgeConfig(bridge *BridgeConfig) error {
	// Parse event bridges
	var eventConfigs []givc_types.EventConfig
	if err := parseJSONEnv(EnvEventProxy, &eventConfigs, false); err != nil {
		return fmt.Errorf("failed to parse %s: %w", EnvEventProxy, err)
	}
	bridge.Events = eventConfigs

	// Parse socket bridges
	var proxyConfigs []givc_types.ProxyConfig
	if err := parseJSONEnv(EnvSocketProxy, &proxyConfigs, false); err != nil {
		return fmt.Errorf("failed to parse %s: %w", EnvSocketProxy, err)
	}
	bridge.Sockets = proxyConfigs

	return nil
}

// parseOptionalCapabilities parses optional service capabilities
func parseOptionalCapabilities(optional *OptionalCapabilities) {
	// Parse exec capability
	if execService, execPresent := os.LookupEnv(EnvExec); execPresent {
		optional.ExecEnabled = execService != "false"
	}

	// Parse wifi capability
	if wifiService, wifiPresent := os.LookupEnv(EnvWifi); wifiPresent {
		optional.WifiEnabled = wifiService != "false"
	}

	// Parse hwid capability
	if hwidService, hwidPresent := os.LookupEnv(EnvHwid); hwidPresent {
		optional.HwidEnabled = hwidService != "false"
		if optional.HwidEnabled {
			optional.HwidInterface = os.Getenv(EnvHwidIface)
		}
	}

	// Parse policyManagement capability
	if policyAgent, policyAgentEnabled := os.LookupEnv(EnvPolicyAgent); policyAgentEnabled {
		optional.PolicyAgentEnabled = policyAgent != "false"
		//TODO: include policy storage and install rules path also
	}

	// Parse notifier capability
	if notifierService, notifierPresent := os.LookupEnv(EnvNotifier); notifierPresent {
		if notifierSocket, notifierSocketPresent := os.LookupEnv(EnvNotifierSocket); notifierSocketPresent {
			optional.NotifierEnabled = (notifierService != "false") && (notifierSocket != "")
			if optional.NotifierEnabled {
				optional.NotifierSocket = notifierSocket
			}
		}
	}
}
