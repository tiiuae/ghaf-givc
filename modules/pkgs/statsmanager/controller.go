// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package statsmanager

import (
	"bufio"
	"cmp"
	"context"
	"fmt"
	"iter"
	"os"
	"slices"
	"strconv"
	"strings"

	stats_api "givc/modules/api/stats"
)

type StatPidFields int

const (
	ProcParentPid StatPidFields = iota
	ProcProcessGroup
	ProcSessionId
	ProcTtyNumber
	ProcTtyProcGroup
	ProcFlags
	ProcMinorFaults
	ProcChildMinorFaults
	ProcMajorFaults
	ProcChildMajorFaults
	ProcUserTime
	ProcSysTime
	ProcChildUserTime
	ProcChildSysTime
	ProcPriority
	ProcNice
	ProcNumThreads
	ProcJiffiesToAlrm
	ProcStartTime
	ProcVmSize
	ProcResidentSetSize
)

type StatFields int

const (
	SysUserTime StatFields = iota
	SysNiceTime
	SysSystemTime
	SysIdleTime
	SysIoWaitTime
	SysIrq
	SysSoftIrq
	SysSteal
	SysGuest
	SysGuestNice
)

type process struct {
	name   string
	state  string
	values []uint64
}

type StatsController struct {
	jiffies   uint64
	processes map[uint64]process
	totals    []uint64
}

func NewController() (*StatsController, error) {
	return &StatsController{jiffies: 0, processes: make(map[uint64]process), totals: make([]uint64, SysGuestNice+1)}, nil
}

// GetMemoryStats retrieves memory statistics from the system.
func (c *StatsController) GetMemoryStats(ctx context.Context) (*stats_api.MemoryStats, error) {

	// Input validation
	if ctx == nil {
		return nil, fmt.Errorf("context cannot be nil")
	}

	file, err := os.Open("/proc/meminfo")

	if err != nil {
		return nil, fmt.Errorf("Could not open memory info")
	}

	lines := bufio.NewScanner(file)
	fields := make(map[string]uint64)

	for lines.Scan() {
		items := strings.FieldsSeq(lines.Text())
		next, stop := iter.Pull(items)
		defer stop()
		key, ok := next()
		if !ok {
			return nil, fmt.Errorf("Unsupported statistics format")
		}
		value, ok := next()
		if !ok {
			return nil, fmt.Errorf("Unsupported statistics format")
		}
		val, err := strconv.ParseUint(value, 10, 64)
		if err != nil {
			return nil, fmt.Errorf("Unsupported statistics format")
		}
		fields[key] = val
	}

	return &stats_api.MemoryStats{
		Total:     fields["MemTotal:"] * 1024,
		Free:      fields["MemFree:"] * 1024,
		Available: fields["MemAvailable:"] * 1024,
		Cached:    fields["Cached:"] * 1024,
	}, nil
}

// GetLoadStats retrieves load statistics from the system.
func (c *StatsController) GetLoadStats(ctx context.Context) (*stats_api.LoadStats, error) {

	// Input validation
	if ctx == nil {
		return nil, fmt.Errorf("context cannot be nil")
	}

	loads, err := os.ReadFile("/proc/loadavg")

	if err != nil {
		return nil, fmt.Errorf("Could not open load info")
	}

	items := strings.Fields(string(loads))

	if len(items) < 3 {
		return nil, fmt.Errorf("Could not parse load info")
	}

	load := make([]float32, 3)
	for i := 0; i < 3; i++ {
		val, err := strconv.ParseFloat(items[i], 32)
		if err != nil {
			return nil, fmt.Errorf("Could not parse load info")
		}
		load[i] = float32(val)
	}

	return &stats_api.LoadStats{
		Load1Min:  load[0],
		Load5Min:  load[1],
		Load15Min: load[2],
	}, nil
}

// GetProcessStats retrieves process statistics from the system.
func (c *StatsController) GetProcessStats(ctx context.Context) (*stats_api.ProcessStats, error) {
	// Input validation
	if ctx == nil {
		return nil, fmt.Errorf("context cannot be nil")
	}

	file, err := os.Open("/proc/stat")
	if err != nil {
		return nil, fmt.Errorf("failed to read stats")
	}

	lines := bufio.NewScanner(file)

	if !lines.Scan() {
		return nil, fmt.Errorf("failed to read stats")
	}

	_, values, found := strings.Cut(lines.Text(), " ")
	if !found {
		return nil, fmt.Errorf("failed to read stats")
	}

	var jiffies uint64
	totals := make([]uint64, SysGuestNice+1)
	i := SysUserTime

	for field := range strings.FieldsSeq(values) {
		jif, err := strconv.ParseUint(field, 10, 64)
		if err != nil {
			return nil, fmt.Errorf("failed to read stats")
		}
		jiffies += jif
		totals[i] = jif
		i++
		if i > SysGuestNice {
			break
		}
	}

	procs, err := os.ReadDir("/proc")
	if err != nil {
		return nil, fmt.Errorf("failed to enumerate processes")
	}

	lastprocesses := c.processes
	c.processes = make(map[uint64]process)
	var changes []process

outer:
	for _, entry := range procs {
		pids := entry.Name()
		pid, err := strconv.ParseUint(pids, 10, 64)

		if err != nil {
			continue
		}

		stat, err := os.ReadFile(fmt.Sprintf("/proc/%s/stat", pids))

		if err != nil {
			continue
		}

		_, namerest, found := strings.Cut(string(stat), "(")
		if !found {
			continue
		}

		name, staterest, found := strings.Cut(namerest, ") ")
		if !found {
			continue
		}

		state, rest, found := strings.Cut(staterest, " ")
		intfields := make([]uint64, ProcResidentSetSize+1)
		i := ProcParentPid
		for field := range strings.FieldsSeq(rest) {
			// TtyProcGroup and Priorty can be negative; not used so just skip
			if i != ProcTtyProcGroup && i != ProcPriority {
				val, err := strconv.ParseUint(field, 10, 64)
				if err != nil {
					continue outer
				}
				intfields[i] = val
			}
			i++
			if i == ProcResidentSetSize+1 {
				break
			}
		}

		if i <= ProcResidentSetSize {
			continue
		}

		prev, found := lastprocesses[pid]
		if found {
			var delta []uint64
			delta = append(delta, intfields...)
			for i := ProcUserTime; i <= ProcChildSysTime; i += 1 {
				delta[i] -= prev.values[i]
			}
			changes = append(changes, process{name, state, delta})
		}

		c.processes[pid] = process{name, state, intfields}
	}

	slices.SortFunc(changes, func(a, b process) int {
		return cmp.Compare(b.values[ProcUserTime]+b.values[ProcSysTime], a.values[ProcUserTime]+a.values[ProcSysTime])
	})

	djiffies := jiffies - c.jiffies
	c.jiffies = jiffies

	usercycles := totals[SysUserTime] - c.totals[SysUserTime]
	syscycles := totals[SysSystemTime] - c.totals[SysSystemTime]
	c.totals = totals

	var cpuProcs []*stats_api.ProcessStat
	for _, proc := range changes[:min(5, len(changes))] {
		userPct := float32(proc.values[ProcUserTime]) * 100 / float32(djiffies)
		sysPct := float32(proc.values[ProcSysTime]) * 100 / float32(djiffies)
		cpuProcs = append(cpuProcs, &stats_api.ProcessStat{Name: proc.name, User: userPct, Sys: sysPct, ResSetSize: proc.values[ProcResidentSetSize]})
	}

	slices.SortFunc(changes, func(a, b process) int {
		return cmp.Compare(b.values[ProcResidentSetSize], a.values[ProcResidentSetSize])
	})

	var memProcs []*stats_api.ProcessStat
	for _, proc := range changes[:min(5, len(changes))] {
		userPct := float32(proc.values[ProcUserTime]) * 100 / float32(djiffies)
		sysPct := float32(proc.values[ProcSysTime]) * 100 / float32(djiffies)
		memProcs = append(cpuProcs, &stats_api.ProcessStat{Name: proc.name, User: userPct, Sys: sysPct, ResSetSize: proc.values[ProcResidentSetSize]})
	}

	return &stats_api.ProcessStats{CpuProcesses: cpuProcs, MemProcesses: memProcs, UserCycles: usercycles, SysCycles: syscycles, TotalCycles: djiffies, Total: 0, Running: 0}, nil
}
