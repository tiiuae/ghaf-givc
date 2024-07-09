// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package hwidmanager

import (
	"context"
	"fmt"
	"os"
	"strings"
)

type HwIdController struct {
	iface string
}

func NewController(iface string) (*HwIdController, error) {
	return &HwIdController{iface: iface}, nil
}

func (c *HwIdController) GetIdentifier(ctx context.Context) (string, error) {

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}

	addr, err := os.ReadFile(fmt.Sprintf("/sys/class/net/%s/address", c.iface))

	if err != nil {
		return "", fmt.Errorf("Could not get identifier")
	}

	return strings.TrimSpace(string(addr)), nil
}
