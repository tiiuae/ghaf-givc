// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package wifimanager

import (
	"context"
	"fmt"
	"os/exec"
	"regexp"
)

type WifiController struct {
}

func NewController() (*WifiController, error) {
	return &WifiController{}, nil
}

func (c *WifiController) GetWifiNetworks(ctx context.Context, NetworkInterface string) (map[string][]string, error) {
	parsedOutput := make(map[string][]string)

	// Input validation
	if ctx == nil {
		return parsedOutput, fmt.Errorf("context cannot be nil")
	}

	nmcliRunCmd := "/run/current-system/sw/bin/nmcli"
	nmcliRunCmd += " -f IN-USE,SSID,SIGNAL,SECURITY "
	nmcliRunCmd += " device wifi "

	// Get wifi list from nix package
	cmd := exec.Command("/bin/sh", "-c", nmcliRunCmd)
	networks, err := cmd.Output()

	if err != nil {
		return parsedOutput, fmt.Errorf("error starting application: %s (%s)", "wifi-list", err)
	}

	parsedOutput["IN-USE"] = []string{}
	parsedOutput["SSID"] = []string{}
	parsedOutput["SIGNAL"] = []string{}
	parsedOutput["SECURITY"] = []string{}

	exp := regexp.MustCompile(`(?m)^(\*?)\s+(.*\S)\s+([0-9]+)\s+(\w+\s?\w+|--)([\s]+)?$`)
	find := exp.FindAllSubmatch(networks, -1)
	for i := range find {
		parsedOutput["IN-USE"] = append(parsedOutput["IN-USE"], string(find[i][1]))
		parsedOutput["SSID"] = append(parsedOutput["SSID"], string(find[i][2]))
		parsedOutput["SIGNAL"] = append(parsedOutput["SIGNAL"], string(find[i][3]))
		parsedOutput["SECURITY"] = append(parsedOutput["SECURITY"], string(find[i][4]))
	}

	return parsedOutput, nil
}
