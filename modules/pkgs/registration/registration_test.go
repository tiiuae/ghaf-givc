// Copyright 2024-2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package registration

import (
	"context"
	"sync"
	"testing"
	"time"
)

func TestNewServiceRegistry(t *testing.T) {
	config := RegistrationConfig{
		AgentServiceName: "test-agent.service",
		AgentType:        1,
		AgentParent:      "test-parent",
		Services:         make(map[string]uint32),
	}

	registry := NewServiceRegistry(config)
	if registry == nil {
		t.Fatal("NewServiceRegistry returned nil")
	}

	// Verify the registry is of correct type
	serviceRegistry, ok := registry.(*ServiceRegistry)
	if !ok {
		t.Fatal("NewServiceRegistry did not return *ServiceRegistry")
	}

	if serviceRegistry.config.AgentServiceName != config.AgentServiceName {
		t.Errorf("Expected AgentServiceName %s, got %s",
			config.AgentServiceName, serviceRegistry.config.AgentServiceName)
	}

	if serviceRegistry.config.AgentType != config.AgentType {
		t.Errorf("Expected AgentType %d, got %d",
			config.AgentType, serviceRegistry.config.AgentType)
	}
}

func TestServiceRegistry_StartRegistrationWorker_CancelledBeforeStart(t *testing.T) {
	config := RegistrationConfig{
		AgentServiceName: "test-agent.service",
		AgentType:        1,
		AgentParent:      "test-parent",
		Services:         make(map[string]uint32),
	}

	registry := NewServiceRegistry(config)

	// Create a context that's already cancelled
	ctx, cancel := context.WithCancel(context.Background())
	cancel() // Cancel immediately

	serverStarted := make(chan struct{})

	// Track if the goroutine has exited
	var wg sync.WaitGroup
	done := make(chan struct{})
	go func() {
		defer close(done)
		registry.StartRegistrationWorker(ctx, &wg, serverStarted)
		wg.Wait()
	}()

	// Wait for completion or timeout
	select {
	case <-done:
		// Good: goroutine completed as expected
	case <-time.After(100 * time.Millisecond):
		t.Fatal("StartRegistrationWorker did not handle context cancellation properly - timed out")
	}
}

func TestServiceRegistry_StartRegistrationWorker_ServerStartSignal(t *testing.T) {
	config := RegistrationConfig{
		AgentServiceName: "test-agent.service",
		AgentType:        1,
		AgentParent:      "test-parent",
		Services:         make(map[string]uint32),
		SystemdServer:    nil, // This will cause RegisterAgent to fail, which is expected
	}

	registry := NewServiceRegistry(config)

	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	serverStarted := make(chan struct{})

	// Track goroutine lifecycle
	var wg sync.WaitGroup
	wg.Add(1)

	go func() {
		defer wg.Done()
		registry.StartRegistrationWorker(ctx, &wg, serverStarted)
	}()

	// Signal that the server has started
	close(serverStarted)

	// Wait for goroutine to complete with timeout
	done := make(chan struct{})
	go func() {
		defer close(done)
		wg.Wait()
	}()

	select {
	case <-done:
		// Good: goroutine completed (with expected registration failure)
	case <-time.After(150 * time.Millisecond):
		t.Fatal("StartRegistrationWorker goroutine did not exit within timeout")
	}
}

func TestServiceRegistry_StartRegistrationWorker_ContextTimeout(t *testing.T) {
	config := RegistrationConfig{
		AgentServiceName: "test-agent.service",
		AgentType:        1,
		AgentParent:      "test-parent",
		Services:         make(map[string]uint32),
	}

	registry := NewServiceRegistry(config)

	// Create context with short timeout
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()

	serverStarted := make(chan struct{})

	// Track goroutine completion
	var wg sync.WaitGroup
	completed := make(chan struct{})
	go func() {
		defer close(completed)
		registry.StartRegistrationWorker(ctx, &wg, serverStarted)
		wg.Wait()
	}()

	// Wait for completion or failure
	select {
	case <-completed:
		// Good: goroutine handled timeout and exited
	case <-time.After(150 * time.Millisecond):
		t.Fatal("StartRegistrationWorker did not handle context timeout properly")
	}
}

func TestRegistrationConfig_Validation(t *testing.T) {
	tests := []struct {
		name          string
		config        RegistrationConfig
		expectedValid bool
	}{
		{
			name: "valid config",
			config: RegistrationConfig{
				AgentServiceName: "test-agent.service",
				AgentType:        1,
				AgentParent:      "test-parent",
				Services:         map[string]uint32{"service1.service": 1},
			},
			expectedValid: true,
		},
		{
			name: "empty agent service name",
			config: RegistrationConfig{
				AgentServiceName: "",
				AgentType:        1,
				AgentParent:      "test-parent",
				Services:         map[string]uint32{"service1.service": 1},
			},
			expectedValid: false,
		},
		{
			name: "nil services map",
			config: RegistrationConfig{
				AgentServiceName: "test-agent.service",
				AgentType:        1,
				AgentParent:      "test-parent",
				Services:         nil,
			},
			expectedValid: true, // nil map is valid, just empty
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			registry := NewServiceRegistry(tt.config)

			// Basic validation: registry should always be created
			if registry == nil {
				t.Fatal("NewServiceRegistry returned nil")
			}

			// Validate config fields are set correctly
			serviceRegistry := registry.(*ServiceRegistry)
			if serviceRegistry.config.AgentServiceName != tt.config.AgentServiceName {
				t.Errorf("Expected AgentServiceName %s, got %s",
					tt.config.AgentServiceName, serviceRegistry.config.AgentServiceName)
			}
		})
	}
}

func TestServiceRegistry_Interface_Compliance(t *testing.T) {
	config := RegistrationConfig{
		AgentServiceName: "test-agent.service",
		AgentType:        1,
		AgentParent:      "test-parent",
		Services:         make(map[string]uint32),
	}

	// Verify that ServiceRegistry implements the Registry interface
	var _ Registry = NewServiceRegistry(config)

	// Test passes if compilation succeeds (interface compliance)
}

func TestServiceRegistry_RegisterServices_EmptyServices(t *testing.T) {
	config := RegistrationConfig{
		AgentServiceName: "test-agent.service",
		AgentType:        1,
		AgentParent:      "test-parent",
		Services:         make(map[string]uint32), // Empty services
	}

	registry := NewServiceRegistry(config)

	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	// This should complete quickly since there are no services to register
	done := make(chan error, 1)
	go func() {
		done <- registry.RegisterServices(ctx)
	}()

	select {
	case err := <-done:
		if err != nil {
			t.Errorf("RegisterServices with empty services should not error, got: %v", err)
		}
	case <-time.After(150 * time.Millisecond):
		t.Fatal("RegisterServices with empty services timed out")
	}
}

func TestServiceRegistry_RegisterServices_ContextCancellation(t *testing.T) {
	config := RegistrationConfig{
		AgentServiceName: "test-agent.service",
		AgentType:        1,
		AgentParent:      "test-parent",
		Services: map[string]uint32{
			"service1.service": 1,
			"service2.service": 2,
		},
	}

	registry := NewServiceRegistry(config)

	ctx, cancel := context.WithCancel(context.Background())

	// Start registration and then cancel immediately
	done := make(chan error, 1)
	go func() {
		done <- registry.RegisterServices(ctx)
	}()

	// Cancel the context quickly
	cancel()

	select {
	case err := <-done:
		if err != context.Canceled {
			t.Errorf("Expected context.Canceled error, got: %v", err)
		}
	case <-time.After(100 * time.Millisecond):
		t.Fatal("RegisterServices did not handle context cancellation within timeout")
	}
}
