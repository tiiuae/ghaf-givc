// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Configuration parsing and validation for the GIVC agent.
package config

import (
	"crypto/tls"

	givc_types "givc/modules/pkgs/types"
)

// AgentConfig holds the complete configuration for the GIVC agent
type AgentConfig struct {
	Identity     IdentityConfig
	Network      NetworkConfig
	Policy       PolicyConfig
	Capabilities CapabilitiesConfig
}

// IdentityConfig
type IdentityConfig struct {
	Type        uint32 // Agent type (using existing UNIT_TYPE_* constants)
	SubType     uint32 // Agent subtype
	Parent      string // Parent agent name
	Name        string // Agent name (derived from transport config)
	ServiceName string // Systemd service name for this agent
}

// PolicyConfig
type PolicyConfig struct {
	PolicyAdminEnabled bool              // Policy admin capability
	PolicyStorePath    string            // Path to the policy configuration file
	PoliciesJson       map[string]string // Policy Json
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
	Units        map[string]uint32
	Applications []givc_types.ApplicationManifest
	Exec         ExecCapability
	Wifi         WifiCapability
	Ctap         CtapCapability
	Hwid         HwidCapability
	Notifier     NotifierCapability
	EventProxy   EventProxyCapability
	SocketProxy  SocketProxyCapability
}

type ExecCapability struct {
	Enabled bool
}

type WifiCapability struct {
	Enabled bool
}

type CtapCapability struct {
	Enabled bool
}

type HwidCapability struct {
	Enabled   bool
	Interface string
}

type NotifierCapability struct {
	Enabled bool
	Socket  string
}

type EventProxyCapability struct {
	Enabled bool
	Events  []givc_types.EventConfig
}

type SocketProxyCapability struct {
	Enabled bool
	Sockets []givc_types.ProxyConfig
}

type RuntimeConfig struct {
	Debug bool
}

type BridgeConfig struct {
	Events  []givc_types.EventConfig
	Sockets []givc_types.ProxyConfig
}
