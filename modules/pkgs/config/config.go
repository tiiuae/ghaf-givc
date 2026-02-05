// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package config

import (
	"crypto/tls"
	"encoding/json"
	"fmt"
	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"
	"os"
)

type IdentityConfig struct {
	Type        uint32 `json:"type"`
	SubType     uint32 `json:"subType"`
	Parent      string `json:"parent"`
	Name        string `json:"name"`
	ServiceName string
}

type NetworkConfig struct {
	AdminEndpoint *givc_types.EndpointConfig `json:"adminEndpoint"`
	AgentEndpoint *givc_types.EndpointConfig `json:"agentEndpoint"`
	Tls           givc_types.TlsConfigJson   `json:"tlsConfig"`
	TlsConfig     *tls.Config
}

type CapabilitiesConfig struct {
	Units      map[string]uint32
	Services   []string `json:"services"`
	VmServices struct {
		AdminVm string   `json:"adminVm"`
		SysVms  []string `json:"systemVms"`
		AppVms  []string `json:"appVms"`
	} `json:"vmServices"`

	Applications []givc_types.ApplicationManifest `json:"applications"`

	Exec struct {
		Enabled bool `json:"enable"`
	} `json:"exec"`

	Wifi struct {
		Enabled bool `json:"enable"`
	} `json:"wifi"`

	Ctap struct {
		Enabled bool `json:"enable"`
	} `json:"ctap"`

	Hwid struct {
		Enabled   bool   `json:"enable"`
		Interface string `json:"interface"`
	} `json:"hwid"`

	Notifier struct {
		Enabled bool   `json:"enable"`
		Socket  string `json:"socket"`
	} `json:"notifier"`

	EventProxy struct {
		Enabled bool                     `json:"enable"`
		Events  []givc_types.EventConfig `json:"events"`
	} `json:"eventProxy"`

	SocketProxy struct {
		Enabled bool                     `json:"enable"`
		Sockets []givc_types.ProxyConfig `json:"sockets"`
	} `json:"socketProxy"`

	Policy givc_types.Policy `json:"policy"`
}

type AgentConfig struct {
	Identity     IdentityConfig     `json:"identity"`
	Network      NetworkConfig      `json:"network"`
	Capabilities CapabilitiesConfig `json:"capabilities"`
}

func LoadConfig(filePath string) (*AgentConfig, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return nil, fmt.Errorf("failed to read config file: %w", err)
	}

	var agentConfig AgentConfig
	if err := json.Unmarshal(data, &agentConfig); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %w", err)
	}

	err = populateAgentConfig(&agentConfig)
	if err != nil {
		return nil, fmt.Errorf("failed to convert config: %w", err)
	}

	return &agentConfig, nil
}

func populateAgentConfig(agentConfig *AgentConfig) error {
	// Service name
	agentConfig.Identity.ServiceName = fmt.Sprintf("givc-%s.service", agentConfig.Identity.Name)

	// Polulate tls config
	if agentConfig.Network.Tls.Enable == true {
		var err error
		agentConfig.Network.TlsConfig, err = givc_util.TlsServerConfig(
			agentConfig.Network.Tls.CaCertPath,
			agentConfig.Network.Tls.CertPath,
			agentConfig.Network.Tls.KeyPath, true)
		if err != nil {
			return fmt.Errorf("failed to create TLS config: %w", err)
		}
	}

	// Populate units
	agentConfig.Capabilities.Units = make(map[string]uint32)
	// Services
	if agentConfig.Capabilities.Services != nil {
		for _, service := range agentConfig.Capabilities.Services {
			agentConfig.Capabilities.Units[service] = agentConfig.Identity.SubType
		}
	}
	// Admin-vm service
	if agentConfig.Capabilities.VmServices.AdminVm != "" {
		agentConfig.Capabilities.Units[agentConfig.Capabilities.VmServices.AdminVm] = givc_types.UNIT_TYPE_ADMVM
	}

	// Sys-vm services
	if agentConfig.Capabilities.VmServices.SysVms != nil {
		for _, vm := range agentConfig.Capabilities.VmServices.SysVms {
			agentConfig.Capabilities.Units[vm] = givc_types.UNIT_TYPE_SYSVM
		}
	}

	// App-vm services
	if agentConfig.Capabilities.VmServices.AppVms != nil {
		for _, vm := range agentConfig.Capabilities.VmServices.AppVms {
			agentConfig.Capabilities.Units[vm] = givc_types.UNIT_TYPE_APPVM
		}
	}

	// Populate admin endpoint
	agentConfig.Network.AdminEndpoint.Services = nil
	agentConfig.Network.AdminEndpoint.TlsConfig = agentConfig.Network.TlsConfig

	// Populate agent endpoint
	var services []string
	services = append(services, agentConfig.Identity.ServiceName)
	for unit := range agentConfig.Capabilities.Units {
		services = append(services, unit)
	}
	agentConfig.Network.AgentEndpoint.TlsConfig = agentConfig.Network.TlsConfig
	agentConfig.Network.AgentEndpoint.Services = services

	return nil

}
