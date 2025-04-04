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

type StatFields int

const (
	ParentPid StatFields = iota
	ProcessGroup
	SessionId
	TtyNumber
	TtyProcGroup
	Flags
	MinorFaults
	ChildMinorFaults
	MajorFaults
	ChildMajorFaults
	UserTime
	SysTime
	ChildUserTime
	ChildSysTime
	Priority
	Nice
	NumThreads
	JiffiesToAlrm
	StartTime
	VmSize
	ResidentSetSize
)

type process struct {
	name   string
	state  string
	values []int64
}

type StatsController struct {
	jiffies   uint64
	processes map[uint64]process
}

func NewController() (*StatsController, error) {
	return &StatsController{jiffies: 0, processes: make(map[uint64]process)}, nil
}

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
	for field := range strings.FieldsSeq(values) {
		jif, err := strconv.ParseUint(field, 10, 64)
		if err != nil {
			return nil, fmt.Errorf("failed to read stats")
		}
		jiffies += jif
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
		intfields := make([]int64, ResidentSetSize+1)
		i := ParentPid
		for field := range strings.FieldsSeq(rest) {
			val, err := strconv.ParseInt(field, 10, 64)
			if err != nil {
				continue outer
			}
			intfields[i] = val
			i++
			if i == ResidentSetSize+1 {
				break
			}
		}

		if i <= ResidentSetSize {
			continue
		}

		prev, found := lastprocesses[pid]
		if found {
			var delta []int64
			delta = append(delta, intfields...)
			for i := UserTime; i <= ChildSysTime; i += 1 {
				delta[i] -= prev.values[i]
			}
			changes = append(changes, process{name, state, delta})
		}

		c.processes[pid] = process{name, state, intfields}
	}

	slices.SortFunc(changes, func(a, b process) int {
		return cmp.Compare(b.values[UserTime]+b.values[SysTime], a.values[UserTime]+a.values[SysTime])
	})

	djiffies := jiffies - c.jiffies
	c.jiffies = jiffies

	var cpuProcs []*stats_api.ProcessStat
	for _, proc := range changes[:min(5, len(changes))] {
		userPct := float32(proc.values[UserTime]) * 100 / float32(djiffies)
		sysPct := float32(proc.values[SysTime]) * 100 / float32(djiffies)
		cpuProcs = append(cpuProcs, &stats_api.ProcessStat{Name: proc.name, User: userPct, Sys: sysPct, ResSetSize: uint64(proc.values[ResidentSetSize])})
	}

	slices.SortFunc(changes, func(a, b process) int {
		return cmp.Compare(b.values[ResidentSetSize], a.values[ResidentSetSize])
	})

	var memProcs []*stats_api.ProcessStat
	for _, proc := range changes[:min(5, len(changes))] {
		userPct := float32(proc.values[UserTime]) * 100 / float32(djiffies)
		sysPct := float32(proc.values[SysTime]) * 100 / float32(djiffies)
		memProcs = append(cpuProcs, &stats_api.ProcessStat{Name: proc.name, User: userPct, Sys: sysPct, ResSetSize: uint64(proc.values[ResidentSetSize])})
	}

	return &stats_api.ProcessStats{CpuProcesses: cpuProcs, MemProcesses: memProcs, Total: 0, Running: 0}, nil
}
