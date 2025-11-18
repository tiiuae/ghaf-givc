// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package hwidmanager

import (
	"context"
	"fmt"
	"net"
	"path/filepath"
)

type HwIdController struct {
	iface string
}

func NewController(iface string) (*HwIdController, error) {
	if iface == "" {
		paths, err := filepath.Glob("/sys/class/net/wl*")
		if err != nil {
			return nil, fmt.Errorf("error querying wireless device name")
		}
		if paths == nil || len(paths) < 1 {
			// if no wireless devices are found, try to find an ethernet device
			paths, err = filepath.Glob("/sys/class/net/en*")
			if err != nil {
				return nil, fmt.Errorf("error querying ethernet device name")
			}
			if paths == nil || len(paths) < 1 {
				return nil, fmt.Errorf("could not find wireless or ethernet device")
			}
		}
		iface = filepath.Base(paths[0])
	}
	return &HwIdController{iface: iface}, nil
}

func (c *HwIdController) GetIdentifier(ctx context.Context) (string, error) {

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}

	// Verify that interface is up
	iface, err := net.InterfaceByName(c.iface)
	if err != nil {
		return "", fmt.Errorf("could not get interface by name")
	}
	if iface.Flags&net.FlagRunning == 0 {
		return "", fmt.Errorf("interface is down, could report unreliable information")
	}

	return iface.HardwareAddr.String(), nil
}
