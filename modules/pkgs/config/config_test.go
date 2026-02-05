// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package config

import (
	"os"
	"path/filepath"
	"testing"

	givc_types "givc/modules/pkgs/types"
)

func TestLoadConfig(t *testing.T) {
	jsonContent := `{
		"identity": {
			"name": "test-agent",
			"type": 1,
			"subType": 2,
			"parent": "test-parent"
		},
		"network": {
			"adminEndpoint": {
				"transport": {
					"name": "admin-vm",
					"addr": "192.168.0.2",
					"port": "9000",
					"protocol": "tcp"
				}
			},
			"agentEndpoint": {
				"transport": {
					"name": "test-agent",
					"addr": "192.168.0.3",
					"port": "9001",
					"protocol": "tcp"
				}
			},
			"tlsConfig": {
				"enable": false
			}
		},
		"capabilities": {
			"services": ["service1.service"],
			"vmServices": {
				"adminVm": "admin-vm.service",
				"systemVms": ["sys-vm.service"],
				"appVms": ["app-vm.service"]
			},
			"applications": [
				{
					"name": "app1",
					"command": "/bin/app1"
				}
			],
			"exec": {"enable": true},
			"wifi": {"enable": false},
			"ctap": {"enable": false},
			"hwid": {"enable": true, "interface": "eth0"},
			"notifier": {"enable": true, "socket": "/tmp/sock"},
			"eventProxy": {"enable": false, "events": []},
			"socketProxy": {"enable": false, "sockets": []},
			"policy": {
				"enable": true,
				"storePath": "/tmp/policy",
				"policyConfig": {"k": "v"}
			}
		}
	}`

	tmpDir := t.TempDir()
	cfgPath := filepath.Join(tmpDir, "config.json")
	if err := os.WriteFile(cfgPath, []byte(jsonContent), 0o600); err != nil {
		t.Fatalf("failed to write temp config: %v", err)
	}

	config, err := LoadConfig(cfgPath)
	if err != nil {
		t.Fatalf("LoadConfig() unexpected error = %v", err)
	}

	// Verify Identity
	if config.Identity.Name != "test-agent" {
		t.Errorf("Identity.Name = %s, want %s", config.Identity.Name, "test-agent")
	}
	if config.Identity.ServiceName != "givc-test-agent.service" {
		t.Errorf("Identity.ServiceName = %s, want %s", config.Identity.ServiceName, "givc-test-agent.service")
	}
	if config.Identity.Type != 1 {
		t.Errorf("Identity.Type = %d, want %d", config.Identity.Type, 1)
	}

	// Verify Network
	if config.Network.AdminEndpoint.Transport.Name != "admin-vm" {
		t.Errorf("Network.AdminEndpoint.Transport.Name = %s, want %s", config.Network.AdminEndpoint.Transport.Name, "admin-vm")
	}
	if config.Network.AgentEndpoint.Transport.Address != "192.168.0.3" {
		t.Errorf("Network.AgentEndpoint.Transport.Address = %s, want %s", config.Network.AgentEndpoint.Transport.Address, "192.168.0.3")
	}
	// Check populated services in AgentEndpoint
	foundService := false
	for _, s := range config.Network.AgentEndpoint.Services {
		if s == "service1.service" {
			foundService = true
			break
		}
	}
	if !foundService {
		t.Errorf("AgentEndpoint.Services missing 'service1.service'")
	}

	// Verify Capabilities
	if len(config.Capabilities.Applications) != 1 {
		t.Errorf("Capabilities.Applications len = %d, want 1", len(config.Capabilities.Applications))
	}
	if config.Capabilities.Exec.Enabled != true {
		t.Errorf("Capabilities.Exec.Enabled = %v, want true", config.Capabilities.Exec.Enabled)
	}
	if config.Capabilities.Hwid.Interface != "eth0" {
		t.Errorf("Capabilities.Hwid.Interface = %s, want eth0", config.Capabilities.Hwid.Interface)
	}

	// Verify Units map population
	if val, ok := config.Capabilities.Units["service1.service"]; !ok || val != 2 { // subType is 2
		t.Errorf("Capabilities.Units['service1.service'] = %d, want 2", val)
	}
	if val, ok := config.Capabilities.Units["admin-vm.service"]; !ok || val != givc_types.UNIT_TYPE_ADMVM {
		t.Errorf("Capabilities.Units['admin-vm.service'] = %d, want %d", val, givc_types.UNIT_TYPE_ADMVM)
	}
}

func TestLoadConfig_InvalidFile(t *testing.T) {
	_, err := LoadConfig("non-existent.json")
	if err == nil {
		t.Error("LoadConfig() expected error for missing file, got nil")
	}
}

func TestLoadConfig_InvalidJSON(t *testing.T) {
	tmpDir := t.TempDir()
	cfgPath := filepath.Join(tmpDir, "invalid.json")
	if err := os.WriteFile(cfgPath, []byte("{invalid-json"), 0o600); err != nil {
		t.Fatalf("failed to write temp config: %v", err)
	}

	_, err := LoadConfig(cfgPath)
	if err == nil {
		t.Error("LoadConfig() expected error for invalid JSON, got nil")
	}
}
