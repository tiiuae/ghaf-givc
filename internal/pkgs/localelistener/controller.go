// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package localelistener

import (
	"context"
	"fmt"
	givc_util "givc/internal/pkgs/utility"
	"os/exec"

	log "github.com/sirupsen/logrus"
)

type LocaleController struct {
}

func NewController() (*LocaleController, error) {
	return &LocaleController{}, nil
}

func (c *LocaleController) SetLocale(ctx context.Context, locale string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}

	if err := exec.Command("localectl", "set-locale", locale).Run(); err != nil {
		log.Warningf("Failed to set locale: %s", err)
	}
	if givc_util.IsRoot() {
		if err := exec.Command("systemctl", "set-environment", "LANG="+locale).Run(); err != nil {
			log.Warningf("Failed to set environment: %s", err)
		}
	} else {
		if err := exec.Command("systemctl", "--user", "set-environment", "LANG="+locale).Run(); err != nil {
			log.Warningf("Failed to set environment: %s", err)
		}
	}

	return nil
}

func (c *LocaleController) SetTimezone(ctx context.Context, timezone string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}

	if err := exec.Command("timedatectl", "set-timezone", timezone).Run(); err != nil {
		log.Warningf("Failed to set timezone: %s", err)
	}

	return nil
}
