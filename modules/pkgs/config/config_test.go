// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package config

import (
	"os"
	"testing"
)

func TestParseJSONEnv(t *testing.T) {
	tests := []struct {
		name     string
		envVar   string
		envValue string
		target   interface{}
		required bool
		wantErr  bool
	}{
		{
			name:     "valid JSON",
			envVar:   "TEST_JSON",
			envValue: `{"name": "test", "port": "9000"}`,
			target: &struct {
				Name string `json:"name"`
				Port string `json:"port"`
			}{},
			required: true,
			wantErr:  false,
		},
		{
			name:     "invalid JSON",
			envVar:   "TEST_INVALID",
			envValue: `{"name": "test", "port":}`,
			target:   &struct{}{},
			required: true,
			wantErr:  true,
		},
		{
			name:     "missing required env var",
			envVar:   "TEST_MISSING",
			envValue: "",
			target:   &struct{}{},
			required: true,
			wantErr:  true,
		},
		{
			name:     "missing optional env var",
			envVar:   "TEST_OPTIONAL",
			envValue: "",
			target:   &struct{}{},
			required: false,
			wantErr:  false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Set up environment
			if tt.envValue != "" {
				os.Setenv(tt.envVar, tt.envValue)
				defer os.Unsetenv(tt.envVar)
			} else {
				os.Unsetenv(tt.envVar)
			}

			err := parseJSONEnv(tt.envVar, tt.target, tt.required)
			if (err != nil) != tt.wantErr {
				t.Errorf("parseJSONEnv() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestParseAgentType(t *testing.T) {
	tests := []struct {
		name     string
		envVar   string
		envValue string
		wantErr  bool
	}{
		{
			name:     "valid agent type",
			envVar:   "TEST_TYPE",
			envValue: "1",
			wantErr:  false,
		},
		{
			name:     "invalid agent type",
			envVar:   "TEST_TYPE_INVALID",
			envValue: "999",
			wantErr:  true,
		},
		{
			name:     "non-numeric agent type",
			envVar:   "TEST_TYPE_NON_NUMERIC",
			envValue: "abc",
			wantErr:  true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			os.Setenv(tt.envVar, tt.envValue)
			defer os.Unsetenv(tt.envVar)

			_, err := parseAgentType(tt.envVar)
			if (err != nil) != tt.wantErr {
				t.Errorf("parseAgentType() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestParseBridgeConfig_Events(t *testing.T) {
	tests := []struct {
		name        string
		envValue    string
		expectError bool
		expectCount int
	}{
		{
			name:        "empty config",
			envValue:    "",
			expectError: false,
			expectCount: 0,
		},
		{
			name:        "valid single event config",
			envValue:    `[{"transport":{"name":"test","address":"127.0.0.1","port":"9001","protocol":"tcp"},"producer":false,"device":"test-device"}]`,
			expectError: false,
			expectCount: 1,
		},
		{
			name:        "valid multiple event configs",
			envValue:    `[{"transport":{"name":"test1","address":"127.0.0.1","port":"9001","protocol":"tcp"},"producer":false,"device":"test-device1"},{"transport":{"name":"test2","address":"127.0.0.1","port":"9002","protocol":"tcp"},"producer":true,"device":"test-device2"}]`,
			expectError: false,
			expectCount: 2,
		},
		{
			name:        "invalid JSON",
			envValue:    `[{"transport":{"name":"test","address"}}`,
			expectError: true,
			expectCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Set up environment
			if tt.envValue != "" {
				os.Setenv(EnvEventProxy, tt.envValue)
				defer os.Unsetenv(EnvEventProxy)
			} else {
				os.Unsetenv(EnvEventProxy)
			}

			var bridge BridgeConfig
			err := parseBridgeConfig(&bridge)

			if tt.expectError {
				if err == nil {
					t.Errorf("parseBridgeConfig() expected error but got none")
				}
			} else {
				if err != nil {
					t.Errorf("parseBridgeConfig() unexpected error = %v", err)
				}
				if len(bridge.Events) != tt.expectCount {
					t.Errorf("parseBridgeConfig() expected %d event configs, got %d", tt.expectCount, len(bridge.Events))
				}
			}
		})
	}
}

func TestParseBridgeConfig_Sockets(t *testing.T) {
	tests := []struct {
		name        string
		envValue    string
		expectError bool
		expectCount int
	}{
		{
			name:        "empty config",
			envValue:    "",
			expectError: false,
			expectCount: 0,
		},
		{
			name:        "valid single proxy config",
			envValue:    `[{"socket":"/tmp/test.sock","server":true,"transport":{"name":"proxy1","address":"127.0.0.1","port":"9001","protocol":"tcp"}}]`,
			expectError: false,
			expectCount: 1,
		},
		{
			name:        "valid multiple proxy configs",
			envValue:    `[{"socket":"/tmp/test1.sock","server":true,"transport":{"name":"proxy1","address":"127.0.0.1","port":"9001","protocol":"tcp"}},{"socket":"/tmp/test2.sock","server":false,"transport":{"name":"proxy2","address":"127.0.0.1","port":"9002","protocol":"tcp"}}]`,
			expectError: false,
			expectCount: 2,
		},
		{
			name:        "invalid JSON",
			envValue:    `[{"socket":"/tmp/test.sock","server":}`,
			expectError: true,
			expectCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Set up environment
			if tt.envValue != "" {
				os.Setenv(EnvSocketProxy, tt.envValue)
				defer os.Unsetenv(EnvSocketProxy)
			} else {
				os.Unsetenv(EnvSocketProxy)
			}

			var bridge BridgeConfig
			err := parseBridgeConfig(&bridge)

			if tt.expectError {
				if err == nil {
					t.Errorf("parseBridgeConfig() expected error but got none")
				}
			} else {
				if err != nil {
					t.Errorf("parseBridgeConfig() unexpected error = %v", err)
				}
				if len(bridge.Sockets) != tt.expectCount {
					t.Errorf("parseBridgeConfig() expected %d socket configs, got %d", tt.expectCount, len(bridge.Sockets))
				}
			}
		})
	}
}

func TestParseIdentityConfig(t *testing.T) {
	tests := []struct {
		name            string
		agentJSON       string
		typeValue       string
		subTypeValue    string
		parentValue     string
		expectError     bool
		expectedName    string
		expectedType    uint32
		expectedSubType uint32
		expectedParent  string
	}{
		{
			name:            "valid identity config",
			agentJSON:       `{"name":"test-agent","addr":"127.0.0.1","port":"9000","protocol":"tcp"}`,
			typeValue:       "1",
			subTypeValue:    "2",
			parentValue:     "parent-agent",
			expectError:     false,
			expectedName:    "test-agent",
			expectedType:    1,
			expectedSubType: 2,
			expectedParent:  "parent-agent",
		},
		{
			name:         "missing agent transport",
			agentJSON:    "",
			typeValue:    "1",
			subTypeValue: "2",
			parentValue:  "",
			expectError:  true,
		},
		{
			name:         "invalid agent type",
			agentJSON:    `{"name":"test-agent","addr":"127.0.0.1","port":"9000","protocol":"tcp"}`,
			typeValue:    "999",
			subTypeValue: "2",
			parentValue:  "",
			expectError:  true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Setup environment
			if tt.agentJSON != "" {
				os.Setenv(EnvAgent, tt.agentJSON)
				defer os.Unsetenv(EnvAgent)
			}
			if tt.typeValue != "" {
				os.Setenv(EnvType, tt.typeValue)
				defer os.Unsetenv(EnvType)
			}
			if tt.subTypeValue != "" {
				os.Setenv(EnvSubtype, tt.subTypeValue)
				defer os.Unsetenv(EnvSubtype)
			}
			if tt.parentValue != "" {
				os.Setenv(EnvParent, tt.parentValue)
				defer os.Unsetenv(EnvParent)
			} else {
				os.Unsetenv(EnvParent)
			}

			var identity IdentityConfig
			err := parseIdentityConfig(&identity)

			if tt.expectError {
				if err == nil {
					t.Errorf("parseIdentityConfig() expected error but got none")
				}
			} else {
				if err != nil {
					t.Errorf("parseIdentityConfig() unexpected error = %v", err)
				}
				if identity.Name != tt.expectedName {
					t.Errorf("parseIdentityConfig() name = %v, want %v", identity.Name, tt.expectedName)
				}
				if identity.Type != tt.expectedType {
					t.Errorf("parseIdentityConfig() type = %v, want %v", identity.Type, tt.expectedType)
				}
				if identity.SubType != tt.expectedSubType {
					t.Errorf("parseIdentityConfig() subtype = %v, want %v", identity.SubType, tt.expectedSubType)
				}
				if identity.Parent != tt.expectedParent {
					t.Errorf("parseIdentityConfig() parent = %v, want %v", identity.Parent, tt.expectedParent)
				}
			}
		})
	}
}

func TestParseConfig_Integration(t *testing.T) {
	tests := []struct {
		name         string
		setupEnv     map[string]string
		expectError  bool
		expectedName string
		expectedType uint32
	}{
		{
			name: "complete valid config",
			setupEnv: map[string]string{
				EnvAgent:       `{"name":"test-agent","addr":"127.0.0.1","port":"9000","protocol":"tcp"}`,
				EnvType:        "1",
				EnvSubtype:     "2",
				EnvParent:      "parent-agent",
				EnvAdminServer: `{"name":"admin-server","addr":"127.0.0.1","port":"8000","protocol":"tcp"}`,
				EnvDebug:       "true",
			},
			expectError:  false,
			expectedName: "test-agent",
			expectedType: 1,
		},
		{
			name: "missing required fields",
			setupEnv: map[string]string{
				EnvType: "1",
			},
			expectError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Setup environment
			for key, value := range tt.setupEnv {
				os.Setenv(key, value)
				defer os.Unsetenv(key)
			}

			config, err := ParseConfig()

			if tt.expectError {
				if err == nil {
					t.Errorf("ParseConfig() expected error but got none")
				}
			} else {
				if err != nil {
					t.Errorf("ParseConfig() unexpected error = %v", err)
				}
				if config.Identity.Name != tt.expectedName {
					t.Errorf("ParseConfig() name = %v, want %v", config.Identity.Name, tt.expectedName)
				}
				if config.Identity.Type != tt.expectedType {
					t.Errorf("ParseConfig() type = %v, want %v", config.Identity.Type, tt.expectedType)
				}
			}
		})
	}
}

func TestParseOptionalCapabilities(t *testing.T) {
	tests := []struct {
		name                string
		setupEnv            map[string]string
		expectedExecEnabled bool
		expectedWifiEnabled bool
		expectedHwidEnabled bool
		expectedHwidIface   string
	}{
		{
			name:                "all capabilities enabled",
			setupEnv:            map[string]string{EnvExec: "true", EnvWifi: "true", EnvHwid: "true", EnvHwidIface: "eth0"},
			expectedExecEnabled: true,
			expectedWifiEnabled: true,
			expectedHwidEnabled: true,
			expectedHwidIface:   "eth0",
		},
		{
			name:                "all capabilities disabled with false",
			setupEnv:            map[string]string{EnvExec: "false", EnvWifi: "false", EnvHwid: "false"},
			expectedExecEnabled: false,
			expectedWifiEnabled: false,
			expectedHwidEnabled: false,
			expectedHwidIface:   "",
		},
		{
			name:                "capabilities enabled with non-false values",
			setupEnv:            map[string]string{EnvExec: "yes", EnvWifi: "enabled", EnvHwid: "on"},
			expectedExecEnabled: true,
			expectedWifiEnabled: true,
			expectedHwidEnabled: true,
			expectedHwidIface:   "",
		},
		{
			name:                "no capabilities set",
			setupEnv:            map[string]string{},
			expectedExecEnabled: false,
			expectedWifiEnabled: false,
			expectedHwidEnabled: false,
			expectedHwidIface:   "",
		},
		{
			name:                "hwid enabled but no interface set",
			setupEnv:            map[string]string{EnvHwid: "true"},
			expectedExecEnabled: false,
			expectedWifiEnabled: false,
			expectedHwidEnabled: true,
			expectedHwidIface:   "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Clean up environment first
			envVars := []string{EnvExec, EnvWifi, EnvHwid, EnvHwidIface}
			for _, env := range envVars {
				os.Unsetenv(env)
			}

			// Setup test environment
			for key, value := range tt.setupEnv {
				os.Setenv(key, value)
				defer os.Unsetenv(key)
			}

			var optional OptionalCapabilities
			parseOptionalCapabilities(&optional)

			if optional.ExecEnabled != tt.expectedExecEnabled {
				t.Errorf("ExecEnabled = %v, want %v", optional.ExecEnabled, tt.expectedExecEnabled)
			}
			if optional.WifiEnabled != tt.expectedWifiEnabled {
				t.Errorf("WifiEnabled = %v, want %v", optional.WifiEnabled, tt.expectedWifiEnabled)
			}
			if optional.HwidEnabled != tt.expectedHwidEnabled {
				t.Errorf("HwidEnabled = %v, want %v", optional.HwidEnabled, tt.expectedHwidEnabled)
			}
			if optional.HwidInterface != tt.expectedHwidIface {
				t.Errorf("HwidInterface = %v, want %v", optional.HwidInterface, tt.expectedHwidIface)
			}
		})
	}
}

func TestParseTLSConfig(t *testing.T) {
	tests := []struct {
		name        string
		tlsJSON     string
		expectError bool
		expectNil   bool
	}{
		{
			name:        "TLS disabled",
			tlsJSON:     `{"enable":false}`,
			expectError: false,
			expectNil:   true,
		},
		{
			name:        "no TLS config",
			tlsJSON:     "",
			expectError: false,
			expectNil:   true,
		},
		{
			name:        "invalid TLS JSON",
			tlsJSON:     `{"enable":true,"caCertPath"}`,
			expectError: true,
			expectNil:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Setup environment
			if tt.tlsJSON != "" {
				os.Setenv(EnvTlsConfig, tt.tlsJSON)
				defer os.Unsetenv(EnvTlsConfig)
			} else {
				os.Unsetenv(EnvTlsConfig)
			}

			tlsConfig, err := parseTLSConfig()

			if tt.expectError {
				if err == nil {
					t.Errorf("parseTLSConfig() expected error but got none")
				}
			} else {
				if err != nil {
					t.Errorf("parseTLSConfig() unexpected error = %v", err)
				}
			}

			if tt.expectNil {
				if tlsConfig != nil {
					t.Errorf("parseTLSConfig() expected nil but got %v", tlsConfig)
				}
			} else {
				if tlsConfig == nil {
					t.Errorf("parseTLSConfig() expected non-nil but got nil")
				}
			}
		})
	}
}
