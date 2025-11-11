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

func TestParseEventConfigs(t *testing.T) {
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

			configs, err := parseEventConfigs()

			if tt.expectError {
				if err == nil {
					t.Errorf("parseEventConfigs() expected error but got none")
				}
			} else {
				if err != nil {
					t.Errorf("parseEventConfigs() unexpected error = %v", err)
				}
				if len(configs) != tt.expectCount {
					t.Errorf("parseEventConfigs() expected %d configs, got %d", tt.expectCount, len(configs))
				}
			}
		})
	}
}

func TestParseProxyConfigs(t *testing.T) {
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

			configs, err := parseProxyConfigs()

			if tt.expectError {
				if err == nil {
					t.Errorf("parseProxyConfigs() expected error but got none")
				}
			} else {
				if err != nil {
					t.Errorf("parseProxyConfigs() unexpected error = %v", err)
				}
				if len(configs) != tt.expectCount {
					t.Errorf("parseProxyConfigs() expected %d configs, got %d", tt.expectCount, len(configs))
				}
			}
		})
	}
}
