// Copyright 2024-2025 TII (SSRC) and the Ghaf contributors
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
				os.Setenv("EVENT_PROXY", tt.envValue)
				defer os.Unsetenv("EVENT_PROXY")
			} else {
				os.Unsetenv("EVENT_PROXY")
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
				os.Setenv("SOCKET_PROXY", tt.envValue)
				defer os.Unsetenv("SOCKET_PROXY")
			} else {
				os.Unsetenv("SOCKET_PROXY")
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
				os.Setenv("AGENT", tt.agentJSON)
				defer os.Unsetenv("AGENT")
			}
			if tt.typeValue != "" {
				os.Setenv("TYPE", tt.typeValue)
				defer os.Unsetenv("TYPE")
			}
			if tt.subTypeValue != "" {
				os.Setenv("SUBTYPE", tt.subTypeValue)
				defer os.Unsetenv("SUBTYPE")
			}
			if tt.parentValue != "" {
				os.Setenv("PARENT", tt.parentValue)
				defer os.Unsetenv("PARENT")
			} else {
				os.Unsetenv("PARENT")
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
				"AGENT":        `{"name":"test-agent","addr":"127.0.0.1","port":"9000","protocol":"tcp"}`,
				"TYPE":         "1",
				"SUBTYPE":      "2",
				"PARENT":       "parent-agent",
				"ADMIN_SERVER": `{"name":"admin-server","addr":"127.0.0.1","port":"8000","protocol":"tcp"}`,
				"DEBUG":        "true",
			},
			expectError:  false,
			expectedName: "test-agent",
			expectedType: 1,
		},
		{
			name: "missing required fields",
			setupEnv: map[string]string{
				"TYPE": "1",
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
