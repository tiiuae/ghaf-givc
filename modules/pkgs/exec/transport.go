// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package exec

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"os/exec"
	"sync"
	"syscall"

	log "github.com/sirupsen/logrus"
	pb "givc/modules/api/exec"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

type process struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	stdout io.ReadCloser
	stderr io.ReadCloser
}

type ExecServer struct {
	pb.UnimplementedExecServer
	processes sync.Map
}

var allowedCommands = map[string]bool{
	"ota-update": true,
	"uptime":     true, // For testing
}

func (s *ExecServer) Name() string {
	return "Exec Server"
}

func (s *ExecServer) RegisterGrpcService(srv *grpc.Server) {
	pb.RegisterExecServer(srv, s)
}

func NewExecServer() (*ExecServer, error) {
	execServer := ExecServer{}

	return &execServer, nil
}

func (s *ExecServer) RunCommand(stream pb.Exec_RunCommandServer) error {
	var proc *process
	var wg sync.WaitGroup

	// Read first request (StartCommand expected)
	req, err := stream.Recv()
	if err != nil {
		return fmt.Errorf("failed to receive start command: %w", err)
	}

	start, ok := req.Command.(*pb.CommandRequest_Start)
	if !ok {
		return fmt.Errorf("expected StartCommand, got something else")
	}

	proc, err = s.startCommand(start.Start, &wg, stream)
	if err != nil {
		return err
	}

	go handleInput(stream, proc)

	// Wait for the process to finish
	err = proc.cmd.Wait()
	wg.Wait()

	exitCode := 0
	if exitErr, ok := err.(*exec.ExitError); ok {
		exitCode = exitErr.ExitCode()
	}
	log.Infof("Streaming Finished event: rc=%d\n", exitCode)
	stream.Send(&pb.CommandResponse{
		Event: &pb.CommandResponse_Finished{
			Finished: &pb.FinishedEvent{ReturnCode: int32(exitCode)},
		},
	})
	return nil

}

func handleInput(stream pb.Exec_RunCommandServer, proc *process) {
	ctx := stream.Context()

	for {
		select {
		case <-ctx.Done():
			// RPC context canceled (client disconnected or deadline exceeded)
			return
		default:
			req, err := stream.Recv()
			if err == io.EOF {
				// Client closed the input stream (stdin closed)
				return
			}
			if err != nil {
				log.Warnf("handleInput: recv error: %v", err)
				return
			}

			switch v := req.Command.(type) {
			case *pb.CommandRequest_Start:
				log.Warnf("handleInput: unexpected Start after process already started")
				return
			case *pb.CommandRequest_Stdin:
				if proc == nil {
					log.Warnf("handleInput: stdin received but process not started")
					return
				}
				if proc.stdin == nil {
					log.Warnf("handleInput: stdin received but stdin not open")
					return
				}
				if v.Stdin.Payload != nil {
					if _, err := proc.stdin.Write(v.Stdin.Payload); err != nil {
						log.Warnf("handleInput: write to stdin failed: %v", err)
						return
					}
				}
				if v.Stdin.GetEof() {
					proc.stdin.Close()
					proc.stdin = nil
				}
			case *pb.CommandRequest_Signal:
				if proc == nil {
					log.Warnf("handleInput: signal received but process not started")
					return
				}
				if err := proc.cmd.Process.Signal(syscall.Signal(v.Signal.Signal)); err != nil {
					log.Warnf("handleInput: sending signal failed: %v", err)
					return
				}
			default:
				log.Warnf("handleInput: unknown command request")
			}
		}
	}
}

func (s *ExecServer) startCommand(req *pb.StartCommand, wg *sync.WaitGroup, stream pb.Exec_RunCommandServer) (*process, error) {
	cmd := exec.Command(req.Command, req.Arguments...)
	if req.WorkingDirectory != nil {
		cmd.Dir = *req.WorkingDirectory
	}
	cmd.Env = append(cmd.Env, flattenEnv(req.EnvVars)...)

	var stdin io.WriteCloser
	var stdout, stderr io.ReadCloser
	var err error

	// Handle stdin: if not provided, bind to /dev/null
	if req.Stdin != nil {
		stdin, err = cmd.StdinPipe()
		if err != nil {
			return nil, err
		}
	} else {
		devNull, err := os.OpenFile("/dev/null", os.O_RDONLY, 0)
		if err != nil {
			return nil, err
		}
		cmd.Stdin = devNull
	}

	// Set up stdout and stderr
	stdout, err = cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	stderr, err = cmd.StderrPipe()
	if err != nil {
		return nil, err
	}

	if err := cmd.Start(); err != nil {
		return nil, err
	}

	wg.Add(2)

	// Send the StartedEvent response
	if err := stream.Send(&pb.CommandResponse{
		Event: &pb.CommandResponse_Started{
			Started: &pb.StartedEvent{Pid: int32(cmd.Process.Pid)},
		},
	}); err != nil {
		return nil, err
	}

	// Stream stdout
	go streamOutput(stdout, stream, wg, func(data []byte, eof bool) *pb.CommandResponse {
		log.Infof("Streaming stdout: %d bytes\n", len(data))
		return &pb.CommandResponse{
			Event: &pb.CommandResponse_Stdout{
				Stdout: &pb.CommandIO{Payload: data, Eof: eof},
			},
		}
	})

	// Stream stderr
	go streamOutput(stderr, stream, wg, func(data []byte, eof bool) *pb.CommandResponse {
		log.Infof("Streaming stderr: %d bytes\n", len(data))
		return &pb.CommandResponse{
			Event: &pb.CommandResponse_Stderr{
				Stderr: &pb.CommandIO{Payload: data, Eof: eof},
			},
		}
	})

	// Write the initial stdin payload if provided
	if req.Stdin != nil && stdin != nil {
		_, err = stdin.Write(req.Stdin)
		if err != nil {
			return nil, fmt.Errorf("failed to write initial stdin: %v", err)
		}
	}

	proc := &process{
		cmd:    cmd,
		stdin:  stdin,
		stdout: stdout,
		stderr: stderr,
	}
	return proc, nil
}

func streamOutput(reader io.ReadCloser, stream pb.Exec_RunCommandServer, wg *sync.WaitGroup, makeResponse func(data []byte, eof bool) *pb.CommandResponse) {
	defer reader.Close()
	defer wg.Done()
	// Create a buffered reader
	bufReader := bufio.NewReader(reader)
	buffer := make([]byte, 1024)
	for {
		n, err := bufReader.Read(buffer)
		if err == io.EOF {
			makeResponse(buffer[:0], true)
			log.Infof("EOF during reading input: %v", err)
			break
		}
		if err != nil {
			log.Errorf("unknown error reading input: %v", err)
			return
		}
		resp := makeResponse(buffer[:n], false)
		if err := stream.Send(resp); err != nil {
			log.Errorf("failed to stream: %v", err)
		}
	}
}

func (s *ExecServer) validateCommand(cmd string) error {
	if _, ok := allowedCommands[cmd]; !ok {
		return status.Errorf(codes.PermissionDenied, "access denied: command %q is not allowed", cmd)
	}
	return nil
}

func flattenEnv(env map[string]string) []string {
	var result []string
	for k, v := range env {
		result = append(result, fmt.Sprintf("%s=%s", k, v))
	}
	return result
}
