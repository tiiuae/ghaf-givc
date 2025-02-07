// Copyright 2024 TII (SSRC) and the Ghaf contributors
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
)

type process struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	stdout io.ReadCloser
	stderr io.ReadCloser
	done   chan struct{}
}

type ExecServer struct {
	pb.UnimplementedExecServer
	processes sync.Map
}

func (s *ExecServer) Name() string {
	return "Wifi Control Server"
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
	/*
		defer func() {
			if proc != nil {
				proc.cmd.Wait()
				close(proc.done)
			}
		}()
	*/

	for {
		req, err := stream.Recv()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}

		switch v := req.Command.(type) {
		case *pb.CommandRequest_Start:
			if proc != nil {
				return fmt.Errorf("process already started")
			}
			proc, err = s.startCommand(v.Start, stream)
			if err != nil {
				return err
			}
		case *pb.CommandRequest_Stdin:
			if proc == nil {
				return fmt.Errorf("process not started")
			}
			_, err = proc.stdin.Write(v.Stdin.Payload)
			if err != nil {
				return err
			}
		case *pb.CommandRequest_Signal:
			if proc == nil {
				return fmt.Errorf("process not started")
			}
			err = proc.cmd.Process.Signal(syscall.Signal(v.Signal.Signal))
			if err != nil {
				return err
			}
		default:
			return fmt.Errorf("unknown command request")
		}
	}
}

func (s *ExecServer) startCommand(req *pb.StartCommand, stream pb.Exec_RunCommandServer) (*process, error) {
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

	done := make(chan struct{})
	if err := cmd.Start(); err != nil {
		return nil, err
	}

	// Send the StartedEvent response
	if err := stream.Send(&pb.CommandResponse{
		Event: &pb.CommandResponse_Started{
			Started: &pb.StartedEvent{Pid: int32(cmd.Process.Pid)},
		},
	}); err != nil {
		return nil, err
	}

	// Stream stdout
	go streamOutput(stdout, stream, func(data []byte) *pb.CommandResponse {
		log.Infof("Streaming stdout: %d bytes\n", len(data))
		return &pb.CommandResponse{
			Event: &pb.CommandResponse_Stdout{
				Stdout: &pb.CommandIO{Payload: data},
			},
		}
	})

	// Stream stderr
	go streamOutput(stderr, stream, func(data []byte) *pb.CommandResponse {
		log.Infof("Streaming stderr: %d bytes\n", len(data))
		return &pb.CommandResponse{
			Event: &pb.CommandResponse_Stderr{
				Stderr: &pb.CommandIO{Payload: data},
			},
		}
	})

	// Wait for the process to finish
	go func() {
		err := cmd.Wait()
		close(done)
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
	}()

	proc := &process{
		cmd:    cmd,
		stdin:  stdin,
		stdout: stdout,
		stderr: stderr,
		done:   done,
	}
	s.processes.Store(cmd.Process.Pid, proc)

	// Write the initial stdin payload if provided
	if req.Stdin != nil && stdin != nil {
		_, err = stdin.Write(req.Stdin)
		if err != nil {
			return nil, fmt.Errorf("failed to write initial stdin: %v", err)
		}
	}
	return proc, nil
}

func streamOutput(reader io.ReadCloser, stream pb.Exec_RunCommandServer, makeResponse func(data []byte) *pb.CommandResponse) {
	defer reader.Close()
	// Create a buffered reader
	bufReader := bufio.NewReader(reader)
	buffer := make([]byte, 1024)
	for {
		n, err := bufReader.Read(buffer)
		if err == io.EOF {
			break
		}
		if err != nil {
			return
		}
		resp := makeResponse(buffer[:n])
		if err := stream.Send(resp); err != nil {
			log.Errorf("failed to stream: %v", err)
		}
	}
}

func flattenEnv(env map[string]string) []string {
	var result []string
	for k, v := range env {
		result = append(result, fmt.Sprintf("%s=%s", k, v))
	}
	return result
}
