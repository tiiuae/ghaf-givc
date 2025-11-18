// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package localelistener

import (
	"context"
	"fmt"
	"os/exec"
	"regexp"

	givc_locale "givc/modules/api/locale"
	givc_util "givc/modules/pkgs/utility"

	log "github.com/sirupsen/logrus"
)

type LocaleController struct {
}

func NewController() (*LocaleController, error) {
	return &LocaleController{}, nil
}

// SetLocale sets the system locale.
func (c *LocaleController) SetLocale(ctx context.Context, assignments []*givc_locale.LocaleAssignment) error {
	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	if len(assignments) == 0 {
		return fmt.Errorf("no locale assignments provided")
	}

	localeArgs := []string{}
	for _, a := range assignments {
		localeArgs = append(localeArgs, fmt.Sprintf("%s=%s", a.Key.String(), a.Value))
	}

	localectlArgs := append([]string{"set-locale"}, localeArgs...)

	if err := exec.Command("localectl", localectlArgs...).Run(); err != nil {
		log.Errorf("Failed to set locale.\nCommand: localectl\nArgs: %#v\nError: %v", localectlArgs, err)
		return err
	}

	if givc_util.IsRoot() {
		systemctlArgs := append([]string{"set-environment"}, localeArgs...)

		if err := exec.Command("systemctl", systemctlArgs...).Run(); err != nil {
			log.Warningf("Failed to set environment. Command: systemctl\nArgs: %#v\nError: %v", systemctlArgs, err)
		}
	} else {
		systemctlArgs := append([]string{"--user", "set-environment"}, localeArgs...)

		if err := exec.Command("systemctl", systemctlArgs...).Run(); err != nil {
			log.Warningf("Failed to set environment. Command: systemctl\nArgs: %#v\nError: %v", systemctlArgs, err)
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
