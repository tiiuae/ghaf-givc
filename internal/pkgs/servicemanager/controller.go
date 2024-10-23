// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package servicemanager

import (
	"context"
	"fmt"
	"strings"
	"syscall"

	givc_app "givc/internal/pkgs/applications"
	types "givc/internal/pkgs/types"
	util "givc/internal/pkgs/utility"

	"github.com/coreos/go-systemd/v22/dbus"
	godbus "github.com/godbus/dbus/v5"
	"github.com/shirou/gopsutil/process"
	log "github.com/sirupsen/logrus"
)

type SystemdController struct {
	conn         *dbus.Conn
	whitelist    []string
	applications []types.ApplicationManifest
}

func NewController(whitelist []string, applications []types.ApplicationManifest) (*SystemdController, error) {
	var err error
	var c SystemdController

	// Create dbus connector
	ctx := context.Background()
	systemMode := util.IsRoot()
	if systemMode {
		c.conn, err = dbus.NewSystemConnectionContext(ctx)
	} else {
		c.conn, err = dbus.NewUserConnectionContext(ctx)
	}
	if err != nil {
		return nil, err
	}

	// Check unit whitelist
	c.whitelist = whitelist
	for _, name := range c.whitelist {
		_, err := c.FindUnit(name)
		if err != nil {
			c.conn.Close()
			return nil, err
		}
	}
	c.applications = applications

	return &c, nil
}

func (c *SystemdController) Close() {
	c.conn.Close()
}

func (c *SystemdController) IsUnitWhitelisted(name string) bool {
	for _, val := range c.whitelist {
		if val == name {
			return true
		}
	}
	return false
}

func (c *SystemdController) FindUnit(name string) ([]dbus.UnitStatus, error) {

	ok := c.IsUnitWhitelisted(name)
	if !ok {
		return nil, fmt.Errorf("unit is not whitelisted")
	}

	var err error
	var units []dbus.UnitStatus
	units, err = c.conn.ListUnitsByNamesContext(context.Background(), []string{name})
	if err != nil {
		return nil, fmt.Errorf("cannot find unit with name %s: %v", name, err)
	}
	if len(units) < 1 {
		return nil, fmt.Errorf("no units found with name %s", name)
	}
	return units, err
}

func (c *SystemdController) FindUnitFiles(name string) ([]dbus.UnitFile, error) {

	ok := c.IsUnitWhitelisted(name)
	if !ok {
		return nil, fmt.Errorf("unit is not whitelisted")
	}

	var err error
	units, err := c.conn.ListUnitFilesByPatternsContext(context.Background(), []string{"enabled"}, []string{name})
	if err != nil {
		return nil, fmt.Errorf("cannot find unit with name %s: %v", name, err)
	}
	if len(units) < 1 {
		return nil, fmt.Errorf("no units found with name %s", name)
	}

	return units, err
}

func (c *SystemdController) FindUnitsByPattern(name string, states string) ([]dbus.UnitStatus, error) {

	ok := c.IsUnitWhitelisted(name)
	if !ok {
		return nil, fmt.Errorf("unit is not whitelisted")
	}

	var err error
	var units []dbus.UnitStatus
	units, err = c.conn.ListUnitsByPatternsContext(context.Background(), []string{states}, []string{name})
	if err != nil {
		return nil, fmt.Errorf("cannot find unit with name %s: %v", name, err)
	}
	if len(units) < 1 {
		return nil, fmt.Errorf("no units found with name %s", name)
	}
	return units, nil
}

func (c *SystemdController) StartUnit(ctx context.Context, name string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	if name == "" {
		return fmt.Errorf("incorrect input, must be unit name")
	}

	// Find unit(s)
	units, err := c.FindUnit(name)
	if err != nil {
		return err
	}

	// Restart unit(s)
	for _, targetUnit := range units {

		// (Re)start unit; 'replace' already queued jobs that may conflict
		ch := make(chan string)
		_, err := c.conn.RestartUnitContext(ctx, targetUnit.Name, "replace", ch)
		if err != nil {
			return err
		}

		status := <-ch
		switch status {
		case "done":
			log.Infof("unit %s (re)start cmd successful\n", name)
		default:
			return fmt.Errorf("failed to (re)start unit %s: %s", name, status)
		}
	}
	// @TODO This only verifies the start job; requires e.g., subscription to track (re)start

	return nil
}

func (c *SystemdController) StopUnit(ctx context.Context, name string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	if name == "" {
		return fmt.Errorf("incorrect input, must be unit name")
	}

	// Find unit(s)
	units, err := c.FindUnit(name)
	if err != nil {
		return err
	}

	// Stop unit(s)
	for _, targetUnit := range units {

		ch := make(chan string)
		_, err := c.conn.StopUnitContext(ctx, targetUnit.Name, "replace", ch)
		if err != nil {
			return err
		}

		status := <-ch
		switch status {
		case "done":
			log.Infof("unit %s stop command successful\n", name)
		default:
			return fmt.Errorf("unit %s stop %s", name, status)
		}
	}
	// @TODO This only verifies the stop job; requires e.g., subscription to track stop

	return nil
}

func (c *SystemdController) KillUnit(ctx context.Context, name string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	if name == "" {
		return fmt.Errorf("incorrect input, must be unit name")
	}

	// Find unit(s)
	units, err := c.FindUnit(name)
	if err != nil {
		return err
	}

	// Kill unit(s)
	for _, targetUnit := range units {
		c.conn.KillUnitContext(ctx, targetUnit.Name, int32(syscall.SIGKILL))
	}

	return nil
}

func (c *SystemdController) FreezeUnit(ctx context.Context, name string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	if name == "" {
		return fmt.Errorf("incorrect input, must be unit name")
	}

	// Find unit(s)
	units, err := c.FindUnit(name)
	if err != nil {
		return err
	}

	// Freeze unit(s)
	for _, targetUnit := range units {
		err := c.conn.FreezeUnit(ctx, targetUnit.Name)
		if err != nil {
			return err
		}
	}

	return nil
}

func (c *SystemdController) UnfreezeUnit(ctx context.Context, name string) error {

	// Input validation
	if ctx == nil {
		return fmt.Errorf("context cannot be nil")
	}
	if name == "" {
		return fmt.Errorf("incorrect input, must be unit name")
	}

	// Find unit(s)
	units, err := c.FindUnit(name)
	if err != nil {
		return err
	}

	// Freeze unit(s)
	for _, targetUnit := range units {
		err := c.conn.ThawUnit(ctx, targetUnit.Name)
		if err != nil {
			return err
		}
	}

	return nil
}

func (c *SystemdController) GetUnitCpuAndMem(ctx context.Context, pid uint32) (float64, float32, error) {

	// Input validation
	if ctx == nil {
		return 0, 0, fmt.Errorf("context cannot be nil")
	}

	// Get process information for the service PID
	p, err := process.NewProcess(int32(pid))
	if err != nil {
		fmt.Printf("Error getting process information for PID %d: %v\n", pid, err)
		return 0, 0, err
	}

	// Get CPU usage percentage
	cpuPercent, err := p.CPUPercent()
	if err != nil {
		fmt.Printf("Error getting CPU usage for PID %d: %v\n", pid, err)
		return 0, 0, err
	}

	// Get memory usage statistics
	memInfo, err := p.MemoryPercent()
	if err != nil {
		fmt.Printf("Error getting memory usage for PID %d: %v\n", pid, err)
		return 0, 0, err
	}

	return cpuPercent, memInfo, nil
}

func (c *SystemdController) GetUnitProperties(ctx context.Context, unitName string) (map[string]interface{}, error) {

	// Input validation
	if ctx == nil {
		return nil, fmt.Errorf("context cannot be nil")
	}
	if unitName == "" {
		return nil, fmt.Errorf("incorrect input, must be unit name")
	}

	// Get unit properties
	props, err := c.conn.GetAllPropertiesContext(ctx, unitName)
	if err != nil {
		return nil, err
	}

	return props, nil
}

func (c *SystemdController) GetUnitPropertyString(ctx context.Context, unitName string, propertyName string) (string, error) {

	// Input validation
	if ctx == nil {
		return "", fmt.Errorf("context cannot be nil")
	}
	if unitName == "" {
		return "", fmt.Errorf("incorrect input, must be unit name")
	}
	if propertyName == "" {
		return "", fmt.Errorf("incorrect input, must be property name")
	}

	// Get unit properties
	prop, err := c.conn.GetUnitPropertyContext(ctx, unitName, propertyName)
	if err != nil {
		return "", err
	}

	propString := strings.Trim(prop.Value.String(), "\"")
	return propString, nil
}

func (c *SystemdController) StartApplication(ctx context.Context, serviceName string, serviceArgs []string) (string, error) {

	cmdFailure := "Command failed."

	// Validate application request
	err := givc_app.ValidateAppUnitRequest(serviceName, serviceArgs, c.applications)
	if err != nil {
		return cmdFailure, err
	}

	// Assemble command
	appName := strings.Split(serviceName, "@")[0]
	appCmd := ""
	for _, app := range c.applications {
		if app.Name == appName {
			appCmd = app.Command
		}
	}
	if appCmd == "" {
		return cmdFailure, fmt.Errorf("application unknown")
	}
	cmd := strings.Split(appCmd, " ")
	if len(cmd) == 0 {
		return cmdFailure, fmt.Errorf("incorrect application string format")
	}

	// Add arguments
	cmd = append(cmd, serviceArgs...)

	// Setup properties
	var props []dbus.Property
	propDescription := dbus.PropDescription("Application service for " + appName)
	propExecStart := dbus.PropExecStart(cmd, false)
	propType := dbus.PropType("exec")
	probEnvironment := dbus.Property{
		Name:  "Environment",
		Value: godbus.MakeVariant([]string{"XDG_CONFIG_DIRS=$XDG_CONFIG_DIRS:/etc/xdg"}),
	}
	props = append(props, propDescription, propExecStart, propType, probEnvironment)

	// Run command as transient service
	jobStatus := make(chan string)
	_, err = c.conn.StartTransientUnitContext(ctx, serviceName, "replace", props, jobStatus)
	if err != nil {
		return cmdFailure, fmt.Errorf("error starting application: %s (%s)", appCmd, err)
	}

	// Check command started
	status := <-jobStatus
	switch status {
	case "done":
		log.Infof("application %s (re)start cmd successful\n", serviceName)
	default:
		return cmdFailure, fmt.Errorf("failed to start app %s: %s", serviceName, status)
	}

	// Whitelist application service
	c.whitelist = append(c.whitelist, serviceName)
	// @TODO remove application from whitelist?

	return "Command successful.", nil
}
