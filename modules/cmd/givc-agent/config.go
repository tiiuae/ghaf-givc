// Copyright 2024-2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Configuration parsing and validation for the GIVC agent.
package main

import (
	"crypto/tls"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"

	givc_app "givc/modules/pkgs/applications"
	givc_exec "givc/modules/pkgs/exec"
	givc_hwidmanager "givc/modules/pkgs/hwidmanager"
	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"
	givc_wifimanager "givc/modules/pkgs/wifimanager"

	log "github.com/sirupsen/logrus"
)

// AgentConfig holds the complete configuration for the GIVC agent
type AgentConfig struct {
	// Core agent configuration
	Agent        givc_types.TransportConfig
	AgentType    uint32
	AgentSubType uint32
	AgentParent  string
	Units        map[string]uint32

	// Service configurations
	Admin        givc_types.TransportConfig
	Applications []givc_types.ApplicationManifest
	TlsConfig    *tls.Config

	// Optional services
	OptionalServices []givc_types.GrpcServiceRegistration

	// External service configurations
	EventConfigs []givc_types.EventConfig
	ProxyConfigs []givc_types.ProxyConfig

	// Debug settings
	Debug bool
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
		{"SERVICES", agentSubType}, // TODO This should be refactor to UNITS
		{"ADMVMS", givc_types.UNIT_TYPE_ADMVM},
		{"SYSVMS", givc_types.UNIT_TYPE_SYSVM},
		{"APPVMS", givc_types.UNIT_TYPE_APPVM},
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
	err := parseJSONEnv("TLS_CONFIG", &tlsConfigJson, false)
	if err != nil {
		return nil, fmt.Errorf("failed to parse TLS_CONFIG: %w", err)
	}

	if tlsConfigJson.Enable {
		return givc_util.TlsServerConfig(tlsConfigJson.CaCertPath, tlsConfigJson.CertPath, tlsConfigJson.KeyPath, true), nil
	}

	return nil, nil
}

// parseOptionalServices creates optional gRPC services based on environment variables
func parseOptionalServices() []givc_types.GrpcServiceRegistration {
	var services []givc_types.GrpcServiceRegistration

	// Parse and create exec server
	execService, execOption := os.LookupEnv("EXEC")
	if execOption && execService != "false" {
		execServer, err := givc_exec.NewExecServer()
		if err != nil {
			log.Errorf("Cannot create exec server: %v", err)
		} else {
			log.Warnf("Enabling exec server - this allows remote execution of arbitrary commands!")
			services = append(services, execServer)
		}
	}

	// Parse and create wifi control server
	wifiService, wifiOption := os.LookupEnv("WIFI")
	if wifiOption && wifiService != "false" {
		wifiControlServer, err := givc_wifimanager.NewWifiControlServer()
		if err != nil {
			log.Errorf("Cannot create wifi control server: %v", err)
		} else {
			log.Infof("Wifi control service enabled")
			services = append(services, wifiControlServer)
		}
	}

	// Parse and create hwid server
	hwidService, hwidOption := os.LookupEnv("HWID")
	if hwidOption && hwidService != "false" {
		hwidIface := ""
		if _, hwidIfOption := os.LookupEnv("HWID_IFACE"); hwidIfOption {
			hwidIface = os.Getenv("HWID_IFACE")
		}
		hwidServer, err := givc_hwidmanager.NewHwIdServer(hwidIface)
		if err != nil {
			log.Errorf("Cannot create hwid server: %v", err)
		} else {
			log.Infof("HWID service enabled")
			services = append(services, hwidServer)
		}
	}

	return services
}

// parseEventConfigs parses event proxy configuration from environment variables
func parseEventConfigs() ([]givc_types.EventConfig, error) {
	var eventConfigs []givc_types.EventConfig
	err := parseJSONEnv("EVENT_PROXY", &eventConfigs, false)
	if err != nil {
		return nil, fmt.Errorf("failed to parse EVENT_PROXY config: %w", err)
	}
	return eventConfigs, nil
}

// parseProxyConfigs parses socket proxy configuration from environment variables
func parseProxyConfigs() ([]givc_types.ProxyConfig, error) {
	var proxyConfigs []givc_types.ProxyConfig
	err := parseJSONEnv("SOCKET_PROXY", &proxyConfigs, false)
	if err != nil {
		return nil, fmt.Errorf("failed to parse SOCKET_PROXY config: %w", err)
	}
	return proxyConfigs, nil
}

// ParseConfig parses and validates the complete agent configuration from environment variables
func ParseConfig() (*AgentConfig, error) {
	config := &AgentConfig{}

	// Parse core agent configuration
	if err := parseJSONEnv("AGENT", &config.Agent, true); err != nil {
		return nil, fmt.Errorf("failed to parse AGENT config: %w", err)
	}

	// Parse agent type and subtype
	agentType, err := parseAgentType("TYPE")
	if err != nil {
		return nil, fmt.Errorf("failed to parse agent type: %w", err)
	}
	config.AgentType = agentType

	agentSubType, err := parseAgentType("SUBTYPE")
	if err != nil {
		return nil, fmt.Errorf("failed to parse agent subtype: %w", err)
	}
	config.AgentSubType = agentSubType

	// Parse agent parent
	config.AgentParent = os.Getenv("PARENT")

	// Parse units
	config.Units = parseUnits(agentSubType)

	// Parse applications
	err = parseJSONEnv("APPLICATIONS", &config.Applications, false)
	if err != nil {
		return nil, fmt.Errorf("failed to parse APPLICATIONS config: %w", err)
	}
	if len(config.Applications) > 0 {
		if err := givc_app.ValidateApplicationManifests(config.Applications); err != nil {
			return nil, fmt.Errorf("error validating application manifests: %w", err)
		}
	}

	// Parse admin server configuration
	if err := parseJSONEnv("ADMIN_SERVER", &config.Admin, true); err != nil {
		return nil, fmt.Errorf("failed to parse ADMIN_SERVER config: %w", err)
	}

	// Parse TLS configuration
	config.TlsConfig, err = parseTLSConfig()
	if err != nil {
		return nil, err
	}

	// Parse optional services
	config.OptionalServices = parseOptionalServices()

	// Parse external service configurations
	config.EventConfigs, err = parseEventConfigs()
	if err != nil {
		return nil, err
	}

	config.ProxyConfigs, err = parseProxyConfigs()
	if err != nil {
		return nil, err
	}

	// Parse debug setting
	config.Debug = os.Getenv("DEBUG") == "true"

	return config, nil
}
