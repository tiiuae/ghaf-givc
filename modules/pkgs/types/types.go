// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package types

import (
	"crypto/tls"

	"google.golang.org/grpc"
)

type UnitType uint32

type UnitStatus struct {
	Name         string
	Description  string
	LoadState    string
	ActiveState  string
	SubState     string
	Path         string
	FreezerState string
}

type TransportConfig struct {
	Name     string `json:"name"`
	Address  string `json:"addr"`
	Port     string `json:"port"`
	Protocol string `json:"protocol"`
}

type EndpointConfig struct {
	Transport TransportConfig
	Services  []string
	TlsConfig *tls.Config
}

type ProxyConfig struct {
	Transport TransportConfig
	Server    bool   `json:"server"`
	Socket    string `json:"socket"`
	TlsConfig *tls.Config
}

type RegistryEntry struct {
	Name      string
	Parent    string
	Type      UnitType
	Transport TransportConfig
	State     UnitStatus
	Watch     bool
}

type ApplicationManifest struct {
	Name        string   `json:"name"`
	Command     string   `json:"command"`
	Args        []string `json:"args,omitempty"`
	Directories []string `json:"directories,omitempty"`
}

type TlsConfigJson struct {
	Enable     bool   `json:"enable"`
	CaCertPath string `json:"caCertPath"`
	CertPath   string `json:"certPath"`
	KeyPath    string `json:"keyPath"`
}

type GrpcServiceRegistration interface {
	Name() string
	RegisterGrpcService(*grpc.Server)
}

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

const (
	APP_ARG_FLAG = "flag"
	APP_ARG_URL  = "url"
	APP_ARG_FILE = "file"
)
