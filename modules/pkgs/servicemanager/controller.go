// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package servicemanager

import (
	"context"
	"fmt"
	"regexp"
	"strings"

	// "sync"
	"syscall"
	"time"

	givc_app "givc/modules/pkgs/applications"
	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"

	"github.com/coreos/go-systemd/v22/dbus"
	godbus "github.com/godbus/dbus/v5"
	"github.com/shirou/gopsutil/process"
	log "github.com/sirupsen/logrus"
)

const (
	NO_WAIT_FOR_MERGE = 0 * time.Second
	WAIT_FOR_MERGE    = 2 * time.Second
)

type SystemdController struct {
	conn         *dbus.Conn
	whitelist    []string
	applications []givc_types.ApplicationManifest
	cancelCtx    context.CancelFunc
}

func NewController(whitelist []string, applications []givc_types.ApplicationManifest) (*SystemdController, error) {
	var err error
	var c SystemdController
	var ctx context.Context

	// Create dbus connector
	ctx, c.cancelCtx = context.WithCancel(context.Background())
	systemMode := givc_util.IsRoot()
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
			c.Close()
			return nil, err
		}
	}

	// Whitelist applications
	if applications != nil {
		c.applications = applications
		for _, app := range c.applications {
			c.whitelist = append(c.whitelist, app.Name)
		}
	}

	return &c, nil
}

func (c *SystemdController) Close() {
	c.conn.Close()
	c.cancelCtx()
}

// IsUnitWhitelisted checks if a unit is whitelisted.
func (c *SystemdController) IsUnitWhitelisted(name string) bool {
	for _, val := range c.whitelist {
		// General units are whitelisted by their full name
		if val == name {
			return true
		}
		// Application instances are whitelisted based
		// on their base name, so we match with regex
		re := regexp.MustCompile(`^` + val + `@[0-9]+\.service$`)
		if re.MatchString(name) {
			return true
		}
	}
	return false
}

// FindUnit returns the status of all units matching the name.
// It performs a whitelist check to ensure the unit is allowed to be queried.
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

// StartUnit starts any systemd unit.
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

	return nil
}

// StopUnit stops a unit by name.
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

	return nil
}

// KillUnit forcefully terminates a unit.
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

// FreezeUnit freezes a unit.
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

// UnfreezeUnit unfreezes (thaw) a unit.
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

// The function getUnitCpuAndMem returns the CPU and memory usage of a unit.
// No check against unit whitelist is performed.
func (c *SystemdController) getUnitCpuAndMem(ctx context.Context, pid uint32) (float64, float32, error) {

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

// The function getUnitProperties returns all properties of a unit as a map.
// No check against unit whitelist is performed.
func (c *SystemdController) getUnitProperties(ctx context.Context, unitName string) (map[string]interface{}, error) {

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

// The function getUnitPropertyString returns the value of a specific property of a unit as a string.
// No check against unit whitelist is performed.
func (c *SystemdController) getUnitPropertyString(ctx context.Context, unitName string, propertyName string) (string, error) {

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

// StartApplication starts an application service with the dynamic arguments.
// Dynamic arguments are validated in accordance with the application manifest.
// The application is started as a transient systemd unit, which means it is not persisted.
// Since applications may be merged into a single unit, this function implements a pre-start
// analysis and a post-start watch mechanism to determine if the application is merged or not.
func (c *SystemdController) StartApplication(ctx context.Context, serviceName string, serviceArgs []string) (*dbus.UnitStatus, error) {

	if ctx == nil {
		return nil, fmt.Errorf("context cannot be nil")
	}

	// Validate application request
	err := givc_app.ValidateAppUnitRequest(serviceName, serviceArgs, c.applications)
	if err != nil {
		return nil, err
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
		return nil, fmt.Errorf("application unknown")
	}
	cmd := strings.Split(appCmd, " ")
	if len(cmd) == 0 {
		return nil, fmt.Errorf("incorrect application string format")
	}
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

	// Since units are merged silently, we are matching running units with the
	// current applications command before we start the application.
	var serviceUnits []dbus.UnitStatus
	runningUnits, err := c.conn.ListUnitsByPatternsContext(ctx, []string{"active"}, []string{"*@*.service"})
	if err != nil {
		return nil, fmt.Errorf("failed to look up merge unit %s", appName)
	}
	log.Infof("Running units: %v", runningUnits)
	for _, unit := range runningUnits {
		// Determine if the unit is a candidate for merging
		candidateUnits, err := c.conn.ListUnitsByPatternsContext(ctx, []string{"active"}, []string{unit.Name})
		if err != nil {
			return nil, fmt.Errorf("failed to look up merge unit %s", appName)
		}
		if len(candidateUnits) != 1 {
			// More than one instance indicates service does not merge, less
			// means no candidate running
			continue
		}

		// Fetch executed command
		prop, err := c.conn.GetServicePropertyContext(ctx, candidateUnits[0].Name, "ExecStart")
		if err != nil {
			return nil, fmt.Errorf("error fetching ExecStart from unit %s", candidateUnits[0].Name)
		}
		candidateExecStart := prop.Value.String()

		// If the applications runs waypipe, we need to check the second argument.
		// This is Ghaf specific. Since commands can be arbitrarily concatenated,
		// there is no automatic way to determine the correct index for comparison.
		idx := 0
		if strings.Contains(cmd[0], "waypipe") && len(cmd) > 1 {
			idx = 1
		}

		// Check if the command matches our current command
		if strings.Contains(candidateExecStart, cmd[idx]) {
			// At this point, we have found a potential merge candidate
			serviceUnits = candidateUnits
			log.Infof("Found merge candidate: %s", serviceUnits[0].Name)
			break
		}
	}

	// Start application
	jobStatus := make(chan string)
	_, err = c.conn.StartTransientUnitContext(ctx, serviceName, "replace", props, jobStatus)
	if err != nil {
		return nil, fmt.Errorf("error starting application: %s (%s)", appCmd, err)
	}

	status := <-jobStatus
	switch status {
	case "done":
		log.Infof("application %s (re)start cmd successful", serviceName)
	default:
		return nil, fmt.Errorf("failed to start app %s: %s", serviceName, status)
	}

	// Since for transient services the unit file is removed, we cannot distinguish if it
	// finished or was merged. Hence we wait and watch its unit status for a while. After
	// the deadline exceeds, we assume the application is running and needs to be registered.
	// Contrary to a simple sleep, this construct allows to return faster if the unit finishes
	// earlier than the watch time, is a first instance, or is merged into another unit.
	// Note that the watch time only impacts the response time to the admin service, the
	// application start time remains constant.
	waitTime := NO_WAIT_FOR_MERGE
	if len(serviceUnits) > 0 {
		// Apply waittime iff a merge candidate was found
		waitTime = WAIT_FOR_MERGE
	}
	deadline := time.After(waitTime)

watch:
	for {
		// Query unit file with exact name. This will always returns a unit status
		runUnit, err := c.conn.ListUnitsByNamesContext(ctx, []string{serviceName})
		if err != nil {
			return nil, fmt.Errorf("failed to watch unit %s", serviceName)
		}
		activeState := runUnit[0].ActiveState

		select {
		case <-ctx.Done():
			return nil, fmt.Errorf("context cancelled during service watch")

		case <-deadline:
			// Service is still active, it needs to be registered with admin
			serviceUnits = runUnit
			break watch

		default:
			switch {
			case activeState == "inactive":
				// Service is reported inactive, which means the unit file was removed.
				switch len(serviceUnits) {
				case 0:
					// Service is inactive and no merge candicate was found, so we report a dead unit.
					serviceUnits = append(serviceUnits, dbus.UnitStatus{
						Name:        serviceName,
						Description: fmt.Sprintf("Exited application: %s", serviceName),
						ActiveState: "inactive",
						SubState:    "dead",
					})
					log.Infof("Failed to find unit with name %s after successful start of application", appName)
				default:
					// Service is inactive, but a merge candidate was found as serviceUnits is not empty.
				}
				break watch

			case activeState == "failed":
				// Service failed to execute successfully
				return nil, fmt.Errorf("application started but failed: %s", serviceName)

			default:
				continue
			}
		}
	}

	return &serviceUnits[0], nil
}
