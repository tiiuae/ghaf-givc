// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package socketproxy

import (
	"fmt"
	"net"
	"os"
	"time"

	log "github.com/sirupsen/logrus"
)

type SocketProxyController struct {
	runAsServer bool
	socket      string
	listener    net.Listener
}

// NewSocketProxyController creates a new SocketProxyController instance.
// It sets up a unix socket for communication and handles ownership and permissions.
func NewSocketProxyController(socket string, runAsServer bool) (*SocketProxyController, error) {

	var listener net.Listener
	var err error
	if !runAsServer {
		// Remove socket file if it exists
		err := os.Remove(socket)
		if err != nil {
			log.Infof("Cannot remove socket: %v", err)
		}

		// Listen on unix socket
		listener, err = net.Listen("unix", socket)
		if err != nil {
			log.Errorf("Unable to listen on unix socket: %v", err)
			return nil, err
		}
	}

	// Block until the socket is created
	_, err = os.Stat(socket)
	for err != nil {
		time.Sleep(500 * time.Millisecond)
		_, err = os.Stat(socket)
	}

	// Change socket owner and permissions to allow any users in group 'users' (gid: 100)
	err = os.Chown(socket, -1, 100)
	if err != nil {
		log.Errorf("Unable to change socket file ownership: %v", err)
	}
	err = os.Chmod(socket, 0770)
	if err != nil {
		log.Errorf("Unable to change socket file permissions: %v", err)
	}

	return &SocketProxyController{socket: socket, runAsServer: runAsServer, listener: listener}, nil
}

// Dial creates a new connection to the unix socket.
func (s *SocketProxyController) Dial() (net.Conn, error) {

	// Dial to the unix socket
	conn, err := net.Dial("unix", s.socket)
	if err != nil {
		log.Errorf("unable to dial unix socket: %v", err)
		return nil, err
	}
	return conn, nil
}

// Accept waits for and returns the next connection to the listener.
func (s *SocketProxyController) Accept() (net.Conn, error) {

	// Accept new connection
	conn, err := s.listener.Accept()
	if err != nil {
		log.Errorf("unable to accept socket connection: %v", err)
		return nil, err
	}
	return conn, nil
}

// Close closes the socket listener.
func (s *SocketProxyController) Close() error {
	if s.listener != nil {
		err := s.listener.Close()
		if err != nil {
			return err
		}
	}
	return nil
}

// Write sends data to the socket connection.
func (s *SocketProxyController) Write(conn net.Conn, data []byte) error {
	n, err := conn.Write(data)
	if err != nil {
		return err
	}
	if n != len(data) {
		return fmt.Errorf("unable to write all data to socket")
	}
	return nil
}

// Read reads data from the socket connection.
func (s *SocketProxyController) Read(conn net.Conn) ([]byte, error) {
	buf := make([]byte, 1024)
	n, err := conn.Read(buf)
	if err != nil {
		return nil, err
	}
	return buf[:n], nil
}
