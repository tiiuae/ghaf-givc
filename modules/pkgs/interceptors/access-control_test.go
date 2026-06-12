// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package interceptors

import (
	"context"
	"crypto/tls"
	"crypto/x509"
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

type MockNestedMsg struct {
	Inner string `protobuf:"bytes,1,opt,name=inner,proto3"`
}

type MockRequest struct {
	SourceVm    string        `protobuf:"bytes,1,opt,name=source_vm,proto3"`
	GrpcModule  string        `protobuf:"bytes,2,opt,name=grpc_module,proto3"`
	GrpcCommand string        `protobuf:"bytes,3,opt,name=grpc_command,proto3"`
	Timeout     int32         `protobuf:"varint,4,opt,name=timeout,proto3"`
	Force       bool          `protobuf:"varint,5,opt,name=force,proto3"`
	CmdParams   []string      `protobuf:"bytes,6,rep,name=cmd_params,proto3"`
	Nested      MockNestedMsg `protobuf:"bytes,7,opt,name=nested,proto3"`
}

func TestAclMapRequestToContext(t *testing.T) {
	req := &MockRequest{
		SourceVm:    "test-vm",
		GrpcModule:  "systemd",
		GrpcCommand: "Start",
		Timeout:     25,
		Force:       true,
		CmdParams:   []string{"web", "prod"},
		Nested:      MockNestedMsg{Inner: "secret"},
	}

	ctxMap := MapRequestToContext(req)

	// Ensure non-protobuf and unexported fields are ignored
	if len(ctxMap) != 7 {
		t.Errorf("Expected exactly 7 fields to be mapped, got %d", len(ctxMap))
	}

	// Test String mapping
	if val, ok := ctxMap[cedartypes.String("SourceVm")]; !ok || val != cedartypes.String("test-vm") {
		t.Errorf("Failed to map SourceVm: got %v", val)
	}
	if val, ok := ctxMap[cedartypes.String("GrpcModule")]; !ok || val != cedartypes.String("systemd") {
		t.Errorf("Failed to map GrpcModule: got %v", val)
	}

	// Test Integer mapping
	if val, ok := ctxMap[cedartypes.String("Timeout")]; !ok || val != cedartypes.Long(25) {
		t.Errorf("Failed to map Timeout: got %v", val)
	}

	// Test Boolean mapping
	if val, ok := ctxMap[cedartypes.String("Force")]; !ok || val != cedartypes.Boolean(true) {
		t.Errorf("Failed to map Force: got %v", val)
	}

	// Test Slice/Set mapping
	if val, ok := ctxMap[cedartypes.String("CmdParams")]; ok {
		expectedSet := cedartypes.NewSet(cedartypes.String("web"), cedartypes.String("prod"))
		if !reflect.DeepEqual(val, expectedSet) {
			t.Errorf("Failed to map CmdParams to Set properly: got %v, expected %v", val, expectedSet)
		}
	} else {
		t.Errorf("Missing CmdParams field")
	}

	// Test Nested Struct/Record mapping
	if val, ok := ctxMap[cedartypes.String("Nested")]; ok {
		expectedRecord := cedartypes.NewRecord(cedartypes.RecordMap{
			cedartypes.String("Inner"): cedartypes.String("secret"),
		})
		if !reflect.DeepEqual(val, expectedRecord) {
			t.Errorf("Failed to map Nested to Record properly: got %v, expected %v", val, expectedRecord)
		}
	} else {
		t.Errorf("Missing Nested field")
	}
}

func TestAclGetSource(t *testing.T) {
	// Case 1: No peer in context
	if src := getSource(context.Background()); src != "unknown" {
		t.Errorf("Expected 'unknown' for empty context, got '%s'", src)
	}

	// Case 2: Peer with IP address (fallback)
	addr := &net.TCPAddr{IP: net.ParseIP("192.168.10.10"), Port: 5555}
	p1 := &peer.Peer{Addr: addr}
	ctx1 := peer.NewContext(context.Background(), p1)
	if src := getSource(ctx1); src != "192.168.10.10" {
		t.Errorf("Expected '192.168.10.10' from IP fallback, got '%s'", src)
	}

	// Case 3: Peer with TLS Certificate (Extracting DNS SAN)
	tlsInfo := credentials.TLSInfo{
		State: tls.ConnectionState{
			PeerCertificates: []*x509.Certificate{
				{DNSNames: []string{"DNS.1:app-vm.local,other"}},
			},
		},
	}
	p2 := &peer.Peer{Addr: addr, AuthInfo: tlsInfo}
	ctx2 := peer.NewContext(context.Background(), p2)
	if src := getSource(ctx2); src != "app-vm.local" {
		t.Errorf("Expected 'app-vm.local' from TLS SAN, got '%s'", src)
	}

	// Case 4: Peer with TLS Certificate (No DNS.1 prefix)
	tlsInfoNoPrefix := credentials.TLSInfo{
		State: tls.ConnectionState{
			PeerCertificates: []*x509.Certificate{
				{DNSNames: []string{"gui-vm.local"}},
			},
		},
	}
	p3 := &peer.Peer{Addr: addr, AuthInfo: tlsInfoNoPrefix}
	ctx3 := peer.NewContext(context.Background(), p3)
	if src := getSource(ctx3); src != "gui-vm.local" {
		t.Errorf("Expected 'gui-vm.local' from TLS SAN without prefix, got '%s'", src)
	}
}

func TestAclPolicy(t *testing.T) {
	// 1. Create a temporary Cedar policy file
	policyContent := `
	// Rule 1: Allow gui-vm to Start applications, but ONLY on app-vm.
	permit (
		principal == Source::"gui-vm",
		action == Command::"Start",
		resource == Module::"systemd"
	) when {
		context.VmName == "app-vm"
	};

	// Rule 2: Allow admin-vm to call ANY method on the systemd module, regardless of the target VM.
	permit (
		principal == Source::"admin-vm",
		action,
		resource == Module::"systemd"
	);

	// Rule 3: Allow metrics-vm to call the GetStats command on the stats module.
	permit (
		principal == Source::"metrics-vm",
		action == Command::"GetStats",
		resource == Module::"stats"
	);
	`
	tempDir := t.TempDir()
	policyPath := filepath.Join(tempDir, "policy.cedar")
	if err := os.WriteFile(policyPath, []byte(policyContent), 0644); err != nil {
		t.Fatalf("Failed to write mock policy: %v", err)
	}

	// 2. Initialize interceptors
	unaryInterceptor, _, err := NewAccessController(policyPath)
	if err != nil {
		t.Fatalf("Failed to initialize AccessController: %v", err)
	}

	// 3. Mock gRPC request structures
	type MockActionReq struct {
		VmName string `protobuf:"bytes,1,opt,name=VmName,proto3"`
	}
	type MockEmptyReq struct{}

	// Dummy gRPC handler that always succeeds
	dummyHandler := func(ctx context.Context, req interface{}) (interface{}, error) {
		return "success", nil
	}

	// Helper to generate context with a specific caller identity
	createCtx := func(principal string) context.Context {
		tlsInfo := credentials.TLSInfo{
			State: tls.ConnectionState{
				PeerCertificates: []*x509.Certificate{{DNSNames: []string{principal}}},
			},
		}
		p := &peer.Peer{Addr: &net.TCPAddr{IP: net.ParseIP("127.0.0.1")}, AuthInfo: tlsInfo}
		return peer.NewContext(context.Background(), p)
	}

	// 4. Define scenarios
	tests := []struct {
		name       string
		principal  string
		method     string
		req        interface{}
		shouldPass bool
	}{
		{
			name:       "Exact Match with Context(Positive)",
			principal:  "gui-vm",
			method:     "/systemd.UnitControl/Start",
			req:        &MockActionReq{VmName: "app-vm"},
			shouldPass: true,
		},
		{
			name:       "Broad Permission(Positive)",
			principal:  "admin-vm",
			method:     "/systemd.UnitControl/Stop",
			req:        &MockActionReq{VmName: "database-vm"},
			shouldPass: true,
		},
		{
			name:       "Context Condition Fails(Negative)",
			principal:  "gui-vm",
			method:     "/systemd.UnitControl/Start",
			req:        &MockActionReq{VmName: "database-vm"},
			shouldPass: false,
		},
		{
			name:       "Wrong Action(Negative)",
			principal:  "gui-vm",
			method:     "/systemd.UnitControl/Stop",
			req:        &MockActionReq{VmName: "app-vm"},
			shouldPass: false,
		},
		{
			name:       "Unknown Principal, implicit deny(Negative)",
			principal:  "compromised-vm",
			method:     "/stats.Metrics/GetStats",
			req:        &MockEmptyReq{},
			shouldPass: false,
		},
	}

	// 5. Run table tests
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
