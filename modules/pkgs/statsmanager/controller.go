// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package statsmanager

import (
	"bufio"
	"context"
	"fmt"
	"os"
	"strconv"
	"strings"

	stats_msg "givc/modules/api/stats_message"
)

type StatsController struct {
}

func NewController() (*StatsController, error) {
	return &StatsController{}, nil
}

func (c *StatsController) GetMemoryStats(ctx context.Context) (*stats_msg.MemoryStats, error) {

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
		items := strings.Fields(lines.Text())
		val, err := strconv.ParseUint(items[1], 10, 64)
		if err != nil {
			return nil, fmt.Errorf("Unsupported statistics format")
		}
		fields[items[0]] = val
	}

	return &stats_msg.MemoryStats{
		Total:     fields["MemTotal:"] * 1024,
		Free:      fields["MemFree:"] * 1024,
		Available: fields["MemAvailable:"] * 1024,
		Cached:    fields["Cached:"] * 1024,
	}, nil
}
