// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package types

import (
	"crypto/tls"

	"google.golang.org/grpc"
)

type UnitType uint32

// UnitStatus represents the status of a systemd unit.
type UnitStatus struct {
	Name         string
	Description  string
	LoadState    string
	ActiveState  string
	SubState     string
	Path         string
	FreezerState string
}

// TransportConfig represents the configuration for a transport layer.
type TransportConfig struct {
	Name     string `json:"name"`
	Address  string `json:"addr"`
	Port     string `json:"port"`
	Protocol string `json:"protocol"`
}

// EndpointConfig represents the configuration for an endpoint, including
// transport settings, services, and TLS configuration.
type EndpointConfig struct {
	Transport TransportConfig
	Services  []string
	TlsConfig *tls.Config
}

// ProxyConfig represents the configuration for a proxy, including transport settings,
// whether it is a server, the socket path, and TLS configuration.
type ProxyConfig struct {
	Transport TransportConfig
	Server    bool   `json:"server"`
	Socket    string `json:"socket"`
	TlsConfig *tls.Config
}

// RegistryEntry represents an entry in the registry, including its name,
// parent, type, transport configuration, state, and whether it should be watched.
type RegistryEntry struct {
	Name      string
	Parent    string
	Type      UnitType
	Transport TransportConfig
	State     UnitStatus
	Watch     bool
}

// ApplicationManifest represents the manifest for an application, including its name,
// command, arguments, and directories.
type ApplicationManifest struct {
	Name        string   `json:"name"`
	Command     string   `json:"command"`
	Args        []string `json:"args,omitempty"`
	Directories []string `json:"directories,omitempty"`
}

// TlsConfigJson represents the JSON configuration for TLS, including whether it is enabled,
// the CA certificate path, the certificate path, and the key path.
type TlsConfigJson struct {
	Enable     bool   `json:"enable"`
	CaCertPath string `json:"caCertPath"`
	CertPath   string `json:"certPath"`
	KeyPath    string `json:"keyPath"`
}

// GrpcServiceRegistration represents a gRPC service registration, including its name
// and the method to register the service with a gRPC server.
type GrpcServiceRegistration interface {
	Name() string
	RegisterGrpcService(*grpc.Server)
}

// UNIT_TYPE_* constants represent types of systemd units to indicate
// their function in the system.
const (
	UNIT_TYPE_HOST_MGR = iota
	UNIT_TYPE_HOST_SVC
	UNIT_TYPE_HOST_APP
	UNIT_TYPE_ADMVM
	UNIT_TYPE_ADMVM_MGR
	UNIT_TYPE_ADMVM_SVC
	UNIT_TYPE_ADMVM_APP
	UNIT_TYPE_SYSVM
	UNIT_TYPE_SYSVM_MGR
	UNIT_TYPE_SYSVM_SVC
	UNIT_TYPE_SYSVM_APP
	UNIT_TYPE_APPVM
	UNIT_TYPE_APPVM_MGR
	UNIT_TYPE_APPVM_SVC
	UNIT_TYPE_APPVM_APP
)

// APP_ARG_* constants represent types of application arguments.
const (
	APP_ARG_FLAG = "flag"
	APP_ARG_URL  = "url"
	APP_ARG_FILE = "file"
)
