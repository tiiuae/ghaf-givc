// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package interceptors

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	execpb "givc/modules/api/exec"
	"givc/modules/api/systemd"
	"net"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	cedartypes "github.com/cedar-policy/cedar-go/types"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/peer"
)

func TestAclMapRequestToContext(t *testing.T) {
	req := &systemd.AppUnitRequest{
		UnitName: "test-application.service",
		Args:     []string{"--flag", "value"},
	}

	ctxMap, err := MapRequestToContext(req)
	if err != nil {
		t.Errorf("Failed to map request to cedar context: %v", err)
	}

	if len(ctxMap) != 2 {
		t.Errorf("Expected exactly 2 fields to be mapped, got %d", len(ctxMap))
	}

	if val, ok := ctxMap[cedartypes.String("UnitName")]; !ok || val != cedartypes.String("test-application.service") {
		t.Errorf("Failed to map unit_name: got %v", val)
	}

	if val, ok := ctxMap[cedartypes.String("Args")]; ok {
		expectedSet := cedartypes.NewSet(cedartypes.String("--flag"), cedartypes.String("value"))
		if !reflect.DeepEqual(val, expectedSet) {
			t.Errorf("Failed to map args to Set properly: got %v, expected %v", val, expectedSet)
		}
	} else {
		t.Errorf("Missing args field")
	}
}

func TestAclGetSource(t *testing.T) {
	// Case 1: No peer in context
	if src, err := getSource(context.Background()); err == nil {
		t.Errorf("Expected error for empty context, got '%s'", src)
	}

	// Case 2: Peer with TLS Certificate (Extracting DNS SAN)
	addr := &net.TCPAddr{IP: net.ParseIP("192.168.10.10"), Port: 5555}
	tlsInfo := credentials.TLSInfo{
		State: tls.ConnectionState{
			PeerCertificates: []*x509.Certificate{
				{DNSNames: []string{"app-vm.local", "other"}},
			},
		},
	}
	p2 := &peer.Peer{Addr: addr, AuthInfo: tlsInfo}
	ctx2 := peer.NewContext(context.Background(), p2)
	if src, err := getSource(ctx2); err != nil || src != "app-vm.local" {
		t.Errorf("Expected 'app-vm.local' from TLS SAN, got '%s' error: %v", src, err)
	}

	// Case 3: Peer with TLS Certificate (No DNS.1 prefix)
	tlsInfoNoPrefix := credentials.TLSInfo{
		State: tls.ConnectionState{
			PeerCertificates: []*x509.Certificate{
				{DNSNames: []string{"gui-vm.local"}},
			},
		},
	}
	p3 := &peer.Peer{Addr: addr, AuthInfo: tlsInfoNoPrefix}
	ctx3 := peer.NewContext(context.Background(), p3)
	if src, err := getSource(ctx3); err != nil || src != "gui-vm.local" {
		t.Errorf("Expected 'gui-vm.local' from TLS SAN without prefix, got '%s'", src)
	}
}

func TestAclPolicy(t *testing.T) {
	policyContent := `
	permit (
		principal == Source::"gui-vm",
		action == Command::"StartApplication",
		resource == Module::"systemd"
	) when {
		context.UnitName == "app-vm.service"
	};
	`
	tempDir := t.TempDir()
	policyPath := filepath.Join(tempDir, "policy.cedar")
	if err := os.WriteFile(policyPath, []byte(policyContent), 0644); err != nil {
		t.Fatalf("Failed to write mock policy: %v", err)
	}

	unaryInterceptor, _, err := NewAccessController(policyPath)
	if err != nil {
		t.Fatalf("Failed to initialize AccessController: %v", err)
	}

	dummyHandler := func(ctx context.Context, req interface{}) (interface{}, error) {
		return "success", nil
	}

	createCtx := func(principal string) context.Context {
		tlsInfo := credentials.TLSInfo{
			State: tls.ConnectionState{
				PeerCertificates: []*x509.Certificate{{DNSNames: []string{principal}}},
			},
		}
		p := &peer.Peer{Addr: &net.TCPAddr{IP: net.ParseIP("127.0.0.1")}, AuthInfo: tlsInfo}
		return peer.NewContext(context.Background(), p)
	}

	tests := []struct {
		name       string
		principal  string
		method     string
		req        interface{}
		shouldPass bool
	}{
		{
			name:      "Exact Match with Context(Positive)",
			principal: "gui-vm",
			method:    "/systemd.UnitControl/StartApplication",
			req: &systemd.AppUnitRequest{
				UnitName: "app-vm.service",
				Args:     []string{},
			},
			shouldPass: true,
		},
		{
			name:      "Broad Permission(Negative)",
			principal: "admin-vm",
			method:    "/systemd.UnitControl/StopApplication",
			req: &systemd.UnitRequest{
				UnitName: "database-vm.service",
			},
			shouldPass: false,
		},
		{
			name:      "Context Condition Fails(Negative)",
			principal: "gui-vm",
			method:    "/systemd.UnitControl/Start",
			req: &systemd.AppUnitRequest{
				UnitName: "database-vm.service",
			},
			shouldPass: false,
		},
		{
			name:      "Wrong Action(Negative)",
			principal: "gui-vm",
			method:    "/systemd.UnitControl/Stop",
			req: &systemd.AppUnitRequest{
				UnitName: "app-vm.service",
			},
			shouldPass: false,
		},
		{
			name:       "Unknown Principal, implicit deny(Negative)",
			principal:  "compromised-vm",
			method:     "/stats.Metrics/GetStats",
			req:        &systemd.UnitRequest{},
			shouldPass: false,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			ctx := createCtx(tc.principal)
			info := &grpc.UnaryServerInfo{FullMethod: tc.method}

			_, err := unaryInterceptor(ctx, tc.req, info, dummyHandler)
			if tc.shouldPass && err != nil {
				t.Errorf("Expected request to be permitted, got error: %v", err)
			} else if !tc.shouldPass && err == nil {
				t.Errorf("Expected request to be denied, but it was permitted")
			}
		})
	}
}

func TestAclPolicyExecCommandSelector(t *testing.T) {
	policyContent := `
	permit (
		principal == Source::"gui-vm",
		action == Command::"RunCommand",
		resource == Module::"exec"
	) when {
		context.service == "Exec" &&
		context.Start.command == "ota-update"
	};
	`
	tempDir := t.TempDir()
	policyPath := filepath.Join(tempDir, "policy.cedar")
	if err := os.WriteFile(policyPath, []byte(policyContent), 0644); err != nil {
		t.Fatalf("Failed to write mock policy: %v", err)
	}

	unaryInterceptor, _, err := NewAccessController(policyPath)
	if err != nil {
		t.Fatalf("Failed to initialize AccessController: %v", err)
	}

	dummyHandler := func(ctx context.Context, req interface{}) (interface{}, error) {
		return "success", nil
	}

	createCtx := func(principal string) context.Context {
		tlsInfo := credentials.TLSInfo{
			State: tls.ConnectionState{
				PeerCertificates: []*x509.Certificate{{DNSNames: []string{principal}}},
			},
		}
		p := &peer.Peer{Addr: &net.TCPAddr{IP: net.ParseIP("127.0.0.1")}, AuthInfo: tlsInfo}
		return peer.NewContext(context.Background(), p)
	}

	tests := []struct {
		name       string
		principal  string
		method     string
		req        interface{}
		shouldPass bool
	}{
		{
			name:      "Structured selector matches ota-update",
			principal: "gui-vm",
			method:    "/exec.Exec/RunCommand",
			req: &execpb.CommandRequest{
				Command: &execpb.CommandRequest_Start{
					Start: &execpb.StartCommand{
						Command: "ota-update",
					},
				},
			},
			shouldPass: true,
		},
		{
			name:      "Structured selector rejects other command",
			principal: "gui-vm",
			method:    "/exec.Exec/RunCommand",
			req: &execpb.CommandRequest{
				Command: &execpb.CommandRequest_Start{
					Start: &execpb.StartCommand{
						Command: "not-ota-update",
					},
				},
			},
			shouldPass: false,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			ctx := createCtx(tc.principal)
			info := &grpc.UnaryServerInfo{FullMethod: tc.method}

			_, err := unaryInterceptor(ctx, tc.req, info, dummyHandler)
			if tc.shouldPass && err != nil {
				t.Errorf("Expected request to be permitted, got error: %v", err)
			} else if !tc.shouldPass && err == nil {
				t.Errorf("Expected request to be denied, but it was permitted")
			}
		})
	}
}
