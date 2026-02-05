// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package config

import (
	"os"
	"path/filepath"
	"testing"

	givc_types "givc/modules/pkgs/types"
)

func TestGetIdentityConfig(t *testing.T) {
	tests := []struct {
		name        string
		json        AgentConfigJSON
		expectError bool
		want        IdentityConfig
	}{
		{
			name: "valid identity",
			json: AgentConfigJSON{
				Name:   "agent-a",
				Type:   "host:application",
				Parent: "parent-a",
			},
			want: IdentityConfig{
				Name:        "agent-a",
				ServiceName: "givc-agent-a.service",
				Type:        givc_types.UNIT_TYPE_HOST_MGR,
				SubType:     givc_types.UNIT_TYPE_HOST_APP,
				Parent:      "parent-a",
			},
		},
		{
			name: "invalid format",
			json: AgentConfigJSON{
				Name: "agent-b",
				Type: "invalidformat",
			},
			expectError: true,
		},
		{
			name: "invalid type",
			json: AgentConfigJSON{
				Name: "agent-c",
				Type: "unknown:service",
			},
			expectError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var identity IdentityConfig
			err := getIdentityConfig(&identity, &tt.json)

			if tt.expectError {
				if err == nil {
					t.Fatalf("getIdentityConfig() expected error")
				}
				return
			}

			if err != nil {
				t.Fatalf("getIdentityConfig() unexpected error = %v", err)
			}

			if identity != tt.want {
				t.Fatalf("getIdentityConfig() got %+v, want %+v", identity, tt.want)
			}
		})
	}
}

func TestGetCapabilitiesConfig(t *testing.T) {
	jsonCapabilities := CapabilitiesConfigJSON{
		Services: []string{"svc-a", "svc-b"},
		VMManager: &VMManagerUnitsConfig{
			Admvms: []string{"admvm-a"},
			Sysvms: []string{"sysvm-a"},
			Appvms: []string{"appvm-a"},
		},
		Applications: []ApplicationConfigJSON{
			{Name: "app1", Command: "/bin/app1", Args: []string{"--flag"}, Directories: []string{"/data"}},
		},
		Exec: &ExecCapabilityJSON{Enabled: true},
		Wifi: &WifiCapabilityJSON{Enabled: false},
		Ctap: &CtapCapabilityJSON{Enabled: false},
		Hwid: &HwidCapabilityJSON{
			Enabled:   true,
			Interface: "eth0",
		},
		Notifier: &NotifierCapabilityJSON{
			Enabled: true,
			Socket:  "/run/not.sock",
		},
	}

	var capabilities CapabilitiesConfig
	err := getCapabilitiesConfig(&capabilities, &jsonCapabilities, givc_types.UNIT_TYPE_HOST_APP)
	if err != nil {
		t.Fatalf("getCapabilitiesConfig() unexpected error = %v", err)
	}

	wantUnits := map[string]uint32{
		"svc-a":   givc_types.UNIT_TYPE_HOST_APP,
		"svc-b":   givc_types.UNIT_TYPE_HOST_APP,
		"admvm-a": givc_types.UNIT_TYPE_ADMVM,
		"sysvm-a": givc_types.UNIT_TYPE_SYSVM,
		"appvm-a": givc_types.UNIT_TYPE_APPVM,
	}

	if len(capabilities.Units) != len(wantUnits) {
		t.Fatalf("expected %d units, got %d", len(wantUnits), len(capabilities.Units))
	}

	for unit, expectedType := range wantUnits {
		if got, ok := capabilities.Units[unit]; !ok || got != expectedType {
			t.Fatalf("unit %s type = %d, want %d", unit, got, expectedType)
		}
	}

	if len(capabilities.Applications) != 1 || capabilities.Applications[0].Name != "app1" {
		t.Fatalf("applications not converted correctly: %+v", capabilities.Applications)
	}

	if !capabilities.Exec.Enabled || capabilities.Wifi.Enabled {
		t.Fatalf("capability flags not converted correctly: Exec=%v, Wifi=%v", capabilities.Exec.Enabled, capabilities.Wifi.Enabled)
	}
	if !capabilities.Hwid.Enabled || capabilities.Hwid.Interface != "eth0" {
		t.Fatalf("hwid options not converted correctly: %+v", capabilities.Hwid)
	}
	if !capabilities.Notifier.Enabled || capabilities.Notifier.Socket != "/run/not.sock" {
		t.Fatalf("notifier options not converted correctly: %+v", capabilities.Notifier)
	}
	if capabilities.Ctap.Enabled {
		t.Fatalf("ctap expected disabled, got enabled")
	}
}

func TestGetNetworkConfig(t *testing.T) {
	jsonConfig := JSONConfig{
		Agent: AgentConfigJSON{
			Name:     "agent-a",
			IPAddr:   "127.0.0.1",
			Port:     "9000",
			Protocol: "tcp",
		},
		AdminServer: AdminServerJSON{
			Name:     "admin-a",
			IPAddr:   "127.0.0.2",
			Port:     "8000",
			Protocol: "tcp",
		},
		TLS: &TLSConfigJSON{Enable: false},
		Capabilities: &CapabilitiesConfigJSON{
			EventProxy: &EventProxyJSON{
				Enabled: true,
				Events: []EventConfigJSON{
					{Name: "ev1", IPAddr: "10.0.0.1", Port: "1000", Protocol: "tcp", Producer: true, Device: "dev1"},
				},
			},
			SocketProxy: &SocketProxyJSON{
				Enabled: true,
				Sockets: []SocketConfigJSON{
					{Name: "proxy1", IPAddr: "10.0.0.2", Port: "1001", Protocol: "udp", Server: true, Socket: "/tmp/sock1"},
				},
			},
		},
	}

	identity := IdentityConfig{
		Name:        "agent-a",
		ServiceName: "givc-agent-a.service",
	}

	capabilities := CapabilitiesConfig{
		Units: map[string]uint32{"svc-a": 1, "svc-b": 2},
	}

	var network NetworkConfig
	err := getNetworkConfig(&network, &jsonConfig, &identity, &capabilities)
	if err != nil {
		t.Fatalf("getNetworkConfig() unexpected error = %v", err)
	}

	if network.TlsConfig != nil {
		t.Fatalf("expected nil TLS config when disabled")
	}

	if network.AdminEndpoint.Transport.Name != "admin-a" || network.AdminEndpoint.Transport.Address != "127.0.0.2" {
		t.Fatalf("admin endpoint not converted correctly: %+v", network.AdminEndpoint.Transport)
	}

	if network.AgentEndpoint.Transport.Name != "agent-a" || network.AgentEndpoint.Transport.Address != "127.0.0.1" {
		t.Fatalf("agent endpoint not converted correctly: %+v", network.AgentEndpoint.Transport)
	}

	wantServices := map[string]struct{}{
		"givc-agent-a.service": {},
		"svc-a":                {},
		"svc-b":                {},
	}
	for _, svc := range network.AgentEndpoint.Services {
		delete(wantServices, svc)
	}
	if len(wantServices) != 0 {
		t.Fatalf("agent endpoint services missing: %v", wantServices)
	}

	if len(network.Bridge.Events) != 1 || network.Bridge.Events[0].Transport.Name != "ev1" {
		t.Fatalf("bridge events not converted correctly: %+v", network.Bridge.Events)
	}
	if len(network.Bridge.Sockets) != 1 || network.Bridge.Sockets[0].Transport.Name != "proxy1" {
		t.Fatalf("bridge sockets not converted correctly: %+v", network.Bridge.Sockets)
	}
}

func TestGetAgentConfig(t *testing.T) {
	jsonConfig := JSONConfig{
		Agent: AgentConfigJSON{
			Name:     "agent-x",
			Type:     "host:application",
			Parent:   "parent-x",
			IPAddr:   "127.0.0.1",
			Port:     "9000",
			Protocol: "tcp",
		},
		AdminServer: AdminServerJSON{
			Name:     "admin-x",
			IPAddr:   "127.0.0.2",
			Port:     "8000",
			Protocol: "tcp",
		},
		TLS: &TLSConfigJSON{Enable: false},
		Policy: &PolicyConfigJSON{
			Enabled:   true,
			StorePath: "/tmp/policy",
			Policies:  map[string]string{"p1": "v1"},
		},
		Capabilities: &CapabilitiesConfigJSON{
			Services: []string{"svc-x"},
			VMManager: &VMManagerUnitsConfig{
				Appvms: []string{"appvm-x"},
			},
			Applications: []ApplicationConfigJSON{
				{Name: "app-x", Command: "/bin/app", Args: []string{"--flag"}},
			},
			Exec: &ExecCapabilityJSON{Enabled: true},
			EventProxy: &EventProxyJSON{
				Enabled: true,
				Events: []EventConfigJSON{
					{Name: "ev-x", IPAddr: "10.0.0.1", Port: "1000", Protocol: "tcp", Producer: true, Device: "dev"},
				},
			},
		},
	}

	config, err := getAgentConfig(&jsonConfig)
	if err != nil {
		t.Fatalf("getAgentConfig() unexpected error = %v", err)
	}

	if config.Identity.Name != "agent-x" || config.Identity.Parent != "parent-x" {
		t.Fatalf("identity not converted correctly: %+v", config.Identity)
	}
	if config.Policy.PolicyStorePath != "/tmp/policy" || !config.Policy.PolicyAdminEnabled {
		t.Fatalf("policy not converted correctly: %+v", config.Policy)
	}
	if len(config.Capabilities.Units) != 2 {
		t.Fatalf("capabilities units not converted correctly: %+v", config.Capabilities.Units)
	}
	if len(config.Network.Bridge.Events) != 1 {
		t.Fatalf("bridge events not converted correctly: %+v", config.Network.Bridge.Events)
	}
}

func TestLoadConfig(t *testing.T) {
	jsonContent := `{
		"agent":{"name":"agent-y","type":"host:application","parent":"parent-y","ipaddr":"127.0.0.1","port":"9000","protocol":"tcp"},
		"adminServer":{"name":"admin-y","ipaddr":"127.0.0.2","port":"8000","protocol":"tcp"},
		"tls":{"enable":false},
		"policy":{"enable":true,"storePath":"/tmp/policy","policies":{"k":"v"}},
		"capabilities":{
			"services":["svc-y"],
			"vmManager":{"admvms":[],"sysvms":[],"appvms":[]},
			"applications":[{"name":"app-y","command":"/bin/app","args":["--flag"],"directories":["/data"]}],
			"exec":{"enable":true},
			"wifi":{"enable":false},
			"ctap":{"enable":false},
			"hwid":{"enable":false,"interface":""},
			"notifier":{"enable":false,"socket":""},
			"eventProxy":{"enable":false,"events":[]},
			"socketProxy":{"enable":false,"sockets":[]}
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

	if config.Identity.Name != "agent-y" || config.Identity.Parent != "parent-y" {
		t.Fatalf("LoadConfig() identity mismatch: %+v", config.Identity)
	}
	if !config.Policy.PolicyAdminEnabled || config.Policy.PolicyStorePath != "/tmp/policy" {
		t.Fatalf("LoadConfig() policy mismatch: %+v", config.Policy)
	}

	if len(config.Capabilities.Applications) != 1 || config.Capabilities.Applications[0].Name != "app-y" {
		t.Fatalf("LoadConfig() applications mismatch: %+v", config.Capabilities.Applications)
	}

	_, err = LoadConfig("non-existent.json")
	if err == nil {
		t.Fatalf("LoadConfig() expected error for missing file")
	}
}

func TestLoadConfigMissingFields(t *testing.T) {
	jsonContent := `{
		"agent":{"name":"agent-z","type":"host:application","ipaddr":"127.0.0.1","port":"9000","protocol":"tcp"},
		"adminServer":{"name":"admin-z","ipaddr":"127.0.0.2","port":"8000","protocol":"tcp"}
	}`

	tmpDir := t.TempDir()
	cfgPath := filepath.Join(tmpDir, "config_minimal.json")
	if err := os.WriteFile(cfgPath, []byte(jsonContent), 0o600); err != nil {
		t.Fatalf("failed to write temp config: %v", err)
	}

	config, err := LoadConfig(cfgPath)
	if err != nil {
		t.Fatalf("LoadConfig() failed with minimal config: %v", err)
	}

	if config.Identity.Name != "agent-z" {
		t.Fatalf("LoadConfig() identity mismatch: %+v", config.Identity)
	}
	if config.Policy.PolicyAdminEnabled {
		t.Fatalf("LoadConfig() expected policy disabled by default")
	}
	if len(config.Capabilities.Units) != 0 {
		t.Fatalf("LoadConfig() expected empty capabilities")
	}
}

func TestUnallowedCapability(t *testing.T) {
	jsonContent := `{
		"agent":{"name":"agent-bad","type":"admin:service","ipaddr":"127.0.0.1","port":"9000","protocol":"tcp"},
		"adminServer":{"name":"admin-bad","ipaddr":"127.0.0.2","port":"8000","protocol":"tcp"},
		"capabilities":{
			"wifi":{"enable":true}
		}
	}`

	tmpDir := t.TempDir()
	cfgPath := filepath.Join(tmpDir, "config_bad.json")
	if err := os.WriteFile(cfgPath, []byte(jsonContent), 0o600); err != nil {
		t.Fatalf("failed to write temp config: %v", err)
	}

	_, err := LoadConfig(cfgPath)
	if err == nil {
		t.Fatalf("LoadConfig() expected error for unallowed capability")
	}
}
