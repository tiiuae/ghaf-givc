// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

/*
	SocketProxyServer is a GRPC service that proxies data between a local socket and a remote GRPC server.

	It can run in two modes, server or client:

		1. As a server: The server waits for a remote client to connect.
		Once connected, it dials to a local socket and proxies data between the remote client and the local socket.

		2. As a client: The client creates a socket listener and waits for a connection to the local socket.
		Once a client (application) connects to the socket, it initiates the connection to the server.

	Both client and server run a GRPC server, and the main routine 'StreamToRemote' which must be run in a different go routine
	than the GRPC server itself.

	As the socket read() function is blocking, the end of a socket connection must be forwarded to the remote, and locally
	closed (here, by closing the socket connection) on both ends. This is done by sending an EOF message to the remote counterpart.

	Both ends unwind and close both socket and GRPC connections when any socket read error occurs, EOF or otherwise.
*/

package socketproxy

import (
	"bytes"
	"fmt"
	"io"
	"net"
	"time"

	givc_socket "givc/modules/api/socket"
	givc_grpc "givc/modules/pkgs/grpc"
	givc_types "givc/modules/pkgs/types"

	"golang.org/x/net/context"
	"golang.org/x/sync/errgroup"
	"google.golang.org/grpc"

	log "github.com/sirupsen/logrus"
)

type SocketProxyServer struct {
	socketController *SocketProxyController
	givc_socket.UnimplementedSocketStreamServer
}

type DataStream interface {
	Recv() (*givc_socket.BytePacket, error)
	Send(*givc_socket.BytePacket) error
	Context() context.Context
}

func (s *SocketProxyServer) Name() string {
	return "Socket Proxy"
}

func (s *SocketProxyServer) RegisterGrpcService(srv *grpc.Server) {
	givc_socket.RegisterSocketStreamServer(srv, s)
}

func NewSocketProxyServer(socket string, runAsServer bool) (*SocketProxyServer, error) {

	// Create a new socket proxy controller
	var err error
	socketController, err := NewSocketProxyController(socket, runAsServer)
	if err != nil {
		return nil, err
	}

	return &SocketProxyServer{
		socketController: socketController,
	}, nil
}

func (s *SocketProxyServer) Close() error {
	return s.socketController.Close()
}

func (s *SocketProxyServer) StreamToRemote(ctx context.Context, cfg *givc_types.EndpointConfig) error {

	defer s.socketController.Close()

	// Setup and dial GRPC client
	grpcClientConn, err := givc_grpc.NewClient(cfg)
	if err != nil {
		return err
	}
	defer grpcClientConn.Close()

	// Create streaming client
	socketStreamClient := givc_socket.NewSocketStreamClient(grpcClientConn)
	if socketStreamClient == nil {
		return fmt.Errorf("failed to create 'NewSocketStreamClient'")
	}

	for {
		select {
		// Return if context is done
		case <-ctx.Done():
			return nil

		// Stream data to remote
		default:

			// Wait for new connection to socket; blocking
			conn, err := s.socketController.Accept()
			if err != nil {
				return err
			}

			// Handle connection
			go func(c net.Conn) {

				// Create new GRPC stream
				stream, err := socketStreamClient.TransferData(ctx)
				for err != nil {
					time.Sleep(1 * time.Second)
					stream, err = socketStreamClient.TransferData(ctx)
				}

				// Stream data between socket and GRPC
				err = s.StreamData(stream, c)
				if err != nil {
					log.Warnf("StreamData exited with: %v", err)
				}

				// Close stream connection
				if stream != nil {
					err := stream.CloseSend()
					if err != nil {
						log.Warnf("Error closing stream: %v", err)
					}
				}

			}(conn)
		}
	}

}

func (s *SocketProxyServer) TransferData(stream givc_socket.SocketStream_TransferDataServer) error {

	if !s.socketController.runAsServer {
		return fmt.Errorf("socket proxy runs as client")
	}

	// Dial to the unix socket
	var err error
	conn, err := s.socketController.Dial()
	if err != nil {
		return err
	}

	return s.StreamData(stream, conn)
}

func (s *SocketProxyServer) StreamData(stream DataStream, conn net.Conn) error {

	group, ctx := errgroup.WithContext(stream.Context())

	// Routine to read from grpc stream and write to socket
	group.Go(func() error {

		for {
			select {
			case <-ctx.Done():
				return nil
			default:

				// Read data from grpc stream
				data, err := stream.Recv()
				if err != nil {
					log.Warnf(">> GRPC failure: %v", err)
					return err
				}
				log.Infof("Recv data: %s", data.GetData())

				// Check for EOF; and close socket connection
				if bytes.Equal(data.GetData(), []byte(io.EOF.Error())) {
					if conn != nil {
						err := conn.Close()
						if err != nil {
							log.Warnf("Error closing socket: %v", err)
						}
					}
					return fmt.Errorf("EOF received")
				}

				// Write data to socket
				err = s.socketController.Write(conn, data.GetData())
				if err != nil {
					log.Warnf("Error writing to socket: %v", err)
					return err
				}
			}
		}
	})

	// Routine to read from socket and write to grpc stream
	group.Go(func() error {

		for {
			select {
			case <-ctx.Done():
				return nil
			default:
				// Read data from socket
				data, err := s.socketController.Read(conn)
				if err != nil {
					// Forward any read error to terminate stream and socket connections on both ends
					log.Infof(">> Socket read error: %v", err)
					message := &givc_socket.BytePacket{
						Data: []byte(io.EOF.Error()),
					}
					err = stream.Send(message)
					if err != nil {
						log.Errorf("failed to send EOF to remote: %v", err)
					}
					return err
				}

				// Send data to grpc stream
				message := &givc_socket.BytePacket{
					Data: data,
				}
				err = stream.Send(message)
				if err != nil {
					return err
				}
				log.Infof("Sent data: %s", data)
			}
		}
	})

	if err := group.Wait(); err != nil {
		log.Infof("Stream exited with: %s", err)
	}

	// Close socket connection
	if conn != nil {
		err := conn.Close()
		if err != nil {
			log.Warnf("Error closing socket: %v", err)
		}
	}

	return nil
}
