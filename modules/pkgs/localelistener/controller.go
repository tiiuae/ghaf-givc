// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package localelistener

import (
	"context"
	"fmt"
	givc_util "givc/modules/pkgs/utility"
	"os/exec"
	"regexp"

	log "github.com/sirupsen/logrus"
)

type LocaleController struct {
}

func NewController() (*LocaleController, error) {
	return &LocaleController{}, nil
}

// SetLocale sets the system locale.
func (c *LocaleController) SetLocale(ctx context.Context, locale string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	re := regexp.MustCompile(`^(?:C|POSIX|[a-z]{2}(?:_[A-Z]{2})?(?:@[a-zA-Z0-9]+)?)(?:\.[-a-zA-Z0-9]+)?$`)
	if !re.MatchString(locale) {
		return fmt.Errorf("invalid locale")
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

// SetTimezone sets the system timezone.
func (c *LocaleController) SetTimezone(ctx context.Context, timezone string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	re := regexp.MustCompile(`^[A-Z][-+a-zA-Z0-9]*(?:/[A-Z][-+a-zA-Z0-9_]*)*$`)
	if !re.MatchString(timezone) {
		return fmt.Errorf("invalid timezone")
	}

	if err := exec.Command("timedatectl", "set-timezone", timezone).Run(); err != nil {
		log.Warningf("Failed to set timezone: %s", err)
	}

	return nil
}
