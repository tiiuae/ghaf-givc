// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package utility

import (
	"fmt"
	"io"
	"net"
	"os"
	"os/user"
	"path/filepath"
	"strings"

	log "github.com/sirupsen/logrus"
)

func IsRoot() bool {
	currentUser, err := user.Current()
	if err != nil {
		log.Fatalf("unable to get current user: %s", err)
	}
	return currentUser.Username == "root"
}

func IsUser(username string) bool {
	currentUser, err := user.Current()
	if err != nil {
		log.Fatalf("unable to get current user: %s", err)
	}
	return currentUser.Username == username
}

func GetHostname() (string, error) {
	hostname, err := os.Hostname()
	if err != nil {
		return "", err
	}
	return hostname, nil
}

func GetIps(hostname string) ([]net.IP, error) {
	addrs, err := net.LookupIP(hostname)
	if err != nil {
		return nil, err
	}
	return addrs, nil
}

func GetFqdn(addrs []net.IP) (string, error) {
	for _, addr := range addrs {
		ipv4 := addr.To4()
		if ipv4 != nil {
			hosts, err := net.LookupAddr(ipv4.String())
			if err != nil || len(hosts) == 0 {
				return "", err
			}
			return strings.TrimSuffix(hosts[0], "."), nil
		}
	}
	return "", fmt.Errorf("cannot find host for ips %v", addrs)
}

func FindCGroupPath(serviceName string) (string, error) {
	cgroupsPath := "/sys/fs/cgroup"

	// Iterate over the cgroup hierarchy
	cgroups, err := os.ReadDir(cgroupsPath)
	if err != nil {
		return "", err
	}

	for _, cgroup := range cgroups {
		if cgroup.IsDir() {
			// Check if the cgroup directory contains the service name
			if strings.Contains(cgroup.Name(), serviceName) {
				// Construct the full cgroup path
				return filepath.Join(cgroupsPath, cgroup.Name()), nil
			}
		}
	}

	return "", fmt.Errorf("cgroup for service %s not found", serviceName)
}

func GetCGroupPathForProcess(pid uint32) (string, error) {
	// Construct the path to the cgroup file for the process
	cgroupFilePath := fmt.Sprintf("/proc/%d/cgroup", pid)

	// Open the cgroup file
	file, err := os.Open(filepath.Clean(cgroupFilePath))
	if err != nil {
		return "", err
	}
	defer file.Close()

	// Read the contents of the cgroup file
	content, err := io.ReadAll(file)
	if err != nil {
		return "", err
	}

	// Parse the cgroup information
	lines := strings.Split(string(content), "\n")
	for _, line := range lines {
		if line != "" {
			parts := strings.Split(line, ":")
			cgroupPath := parts[2]

			// Return the cgroup path for the first hierarchy
			return cgroupPath, nil
		}
	}

	return "", fmt.Errorf("cgroup information not found for process %d", pid)
}

func GetInterfaceIpv4(ifname string) (string, error) {

	ief, err := net.InterfaceByName(ifname)
	if err != nil {
		return "", fmt.Errorf("could not find interface %s", ifname)
	}
	addrs, err := ief.Addrs()
	if err != nil || len(addrs) < 1 {
		return "", fmt.Errorf("could not find ips for interface %s", ifname)
	}

	switch ip := addrs[0].(type) {
	case *net.IPAddr:
		if ipv4 := ip.IP.To4(); ipv4 != nil {
			return ip.IP.String(), nil
		}
	case *net.IPNet:
		if ipv4 := ip.IP.To4(); ipv4 != nil {
			return ip.IP.String(), nil
		}
	}

	return "", fmt.Errorf("could not find ip for interface %s", ifname)
}

func GetInterfaceIpv4Dns(hostname string) (string, error) {

	ips, err := net.LookupIP(hostname)
	if err != nil || len(ips) < 1 {
		return "", fmt.Errorf("could not find ip for hostname %s", hostname)
	}
	for _, ip := range ips {
		if ipv4 := ip.To4(); ipv4 != nil {
			return string(ipv4), nil
		}
	}
	return "", fmt.Errorf("could not find ip for hostname %s", hostname)
}

func GetOutboundIP() net.IP {
	conn, err := net.Dial("udp", "8.8.8.8:80")
	if err != nil {
		log.Fatal(err)
	}
	defer conn.Close()

	localAddr := conn.LocalAddr().(*net.UDPAddr)

	return localAddr.IP
}

func CheckStringInArray(element string, array []string) bool {
	for _, e := range array {
		if e == element {
			return true
		}
	}
	return false
}
