// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package systemmanager

import (
	"github.com/qmuntal/stateless"
)

// System states
const (
	STATE_INIT          = "Initializing system"
	STATE_REGISTER_HOST = "System waiting to register host"
	STATE_REGISTER_VMS  = "System waiting to register virtual machines"
	STATE_RUN           = "System is running"
)

// System triggers
const (
	TIRGGER_INIT_COMPLETE   = "Start registering system components"
	TRIGGER_HOST_REGISTERED = "Host registered"
	TRIGGER_VMS_REGISTERED  = "System-VMs registered"
)

func (svc *AdminService) InitSystemStateMachine() *stateless.StateMachine {
	statemachine := stateless.NewStateMachine(STATE_INIT)

	// Configure state machine
	statemachine.Configure(STATE_INIT).Permit(TIRGGER_INIT_COMPLETE, STATE_REGISTER_HOST)
	statemachine.Configure(STATE_REGISTER_HOST).Permit(TRIGGER_HOST_REGISTERED, STATE_REGISTER_VMS)
	statemachine.Configure(STATE_REGISTER_VMS).Permit(TRIGGER_VMS_REGISTERED, STATE_RUN)

	return statemachine
}
