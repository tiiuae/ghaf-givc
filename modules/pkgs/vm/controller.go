// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package vm

import (
	"context"
	"fmt"
	"regexp"

	givc_vm "givc/modules/api/vm"

	"github.com/godbus/dbus/v5"
	log "github.com/sirupsen/logrus"
)

const (
	MemManagerIface = "ae.tii.MemManager"
	VmIface         = MemManagerIface + ".VM"
	MinimumProperty = VmIface + ".Minimum"
	MaximumProperty = VmIface + ".Maximum"
	MaxSizeProperty = VmIface + ".MaxSize"
	GetStatsMethod  = VmIface + ".GetStats"
)

type VmController struct {
	conn *dbus.Conn
	re   *regexp.Regexp
}

func NewController() (*VmController, error) {
	var err error
	var c VmController

	c.conn, err = dbus.ConnectSystemBus()
	if err != nil {
		return nil, fmt.Errorf("failed to connect to system bus: %s", err)
	}

	c.re = regexp.MustCompile("[^A-Za-z0-9]")

	return &c, nil
}

func (c *VmController) Close() {
	err := c.conn.Close()
	if err != nil {
		log.Warnf("[VmController] failed to close connection: %s", err)
	}
}

func (c *VmController) objectPath(vm string) dbus.ObjectPath {
	vmPath := "/" + c.re.ReplaceAllLiteralString(vm, "_")
	return dbus.ObjectPath(vmPath)
}

func (c *VmController) object(vm string) dbus.BusObject {
	vmObj := c.objectPath(vm)
	return c.conn.Object(MemManagerIface, vmObj)
}

func (c *VmController) VMSize(ctx context.Context, vm string, minimum *uint64, maximum *uint64) (*givc_vm.VMSizeResponse, error) {
	vmObj := c.object(vm)
	var ret givc_vm.VMSizeResponse
	if minimum != nil {
		if err := vmObj.SetProperty(MinimumProperty, *minimum); err != nil {
			return nil, err
		}
		ret.Minimum = *minimum
	} else {
		val, err := vmObj.GetProperty(MinimumProperty)
		if err != nil {
			return nil, err
		}
		uval, _ := val.Value().(uint64)
		ret.Minimum = uval
	}
	if maximum != nil {
		if err := vmObj.SetProperty(MaximumProperty, *maximum); err != nil {
			return nil, err
		}
		ret.Maximum = *maximum
	} else {
		val, err := vmObj.GetProperty(MaximumProperty)
		if err != nil {
			return nil, err
		}
		uval, _ := val.Value().(uint64)
		ret.Maximum = uval
	}

	val, err := vmObj.GetProperty(MaxSizeProperty)
	if err != nil {
		return nil, err
	}
	uval, _ := val.Value().(uint64)
	ret.Size = uval

	return &ret, nil
}

func (c *VmController) VMStats(ctx context.Context, vm string) (*map[string]uint64, error) {
	vmObj := c.object(vm)
	var stats map[string]uint64

	err := vmObj.Call(GetStatsMethod, 0).Store(&stats)
	if err != nil {
		return nil, err
	}

	return &stats, nil
}
