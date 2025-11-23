// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package notifier

import (
	"fmt"
	"net"
	"os"
	"path/filepath"
	"sync"

	log "github.com/sirupsen/logrus"
)

type SocketController struct {
	socketDir string
}

// NewSocketController creates a new SocketController instance.
func NewSocketController(socketDir string) (*SocketController, error) {
	return &SocketController{socketDir: socketDir}, nil
}

// BroadcastNotification sends a JSON message to all discovered Unix sockets in the given socket directory.
func (s *SocketController) BroadcastNotification(jsonMessage []byte) error {
	// Read sockets directory
	entries, err := os.ReadDir(s.socketDir)
	if err != nil {
		log.Errorf("Could not read directory '%s': %v", s.socketDir, err)
		return err
	}

	// Discover all sockets in socket directory
	var sockets []string
	for _, entry := range entries {
		fullPath := filepath.Join(s.socketDir, entry.Name())

		// Get file metadata
		fileInfo, err := os.Stat(fullPath)
		if err != nil {
			log.Warnf("Could not stat file '%s', skipping: %v", fullPath, err)
			continue
		}

		// Check if the file is a socket
		if fileInfo.Mode()&os.ModeSocket != 0 {
			log.Infof("Found socket: %s", fullPath)
			sockets = append(sockets, fullPath)
		}
	}

	// No sockets found - this means no active users. This is expected if
	// no user is logged in.
	if len(sockets) == 0 {
		log.Infof("No sockets found in directory %s - Exiting.", s.socketDir)
		return nil
	}

	// Broadcast notification to all sockets concurrently
	var wg sync.WaitGroup
	for _, socketPath := range sockets {
		wg.Add(1)
		go func(path string) {
			defer wg.Done()
			err := sendMessageToSocket(path, jsonMessage)
			if err != nil {
				log.Warnf("Error sending message to socket '%s': %v", path, err)
			}
		}(socketPath)
	}
	wg.Wait()

	return nil
}

// sendMessageToSocket sends a JSON message to a specified Unix socket.
func sendMessageToSocket(socketPath string, data []byte) error {

	// Dial to the unix socket
	conn, err := net.Dial("unix", socketPath)
	if err != nil {
		return err
	}
	defer conn.Close()

	// Write the JSON message to the socket
	n, err := conn.Write(data)
	if err != nil {
		return err
	}
	if n != len(data) {
		return fmt.Errorf("unable to write all data to socket")
	}

	return nil
}
