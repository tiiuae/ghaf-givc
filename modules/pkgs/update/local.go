// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package update

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"sync"
	"time"

	pb "givc/modules/api/update"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

const localChunkSize = 256 * 1024
const localReceiveIdle = 30 * time.Second

func localRole(want string) error {
	if os.Getenv("GHAF_LOCAL_UPDATE_ROLE") != want {
		return status.Error(codes.PermissionDenied, "local update operation is unavailable on this agent")
	}
	return nil
}

func localID(id string) error {
	if len(id) != 32 {
		return status.Error(codes.InvalidArgument, "invalid local update ID")
	}
	for _, c := range id {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return status.Error(codes.InvalidArgument, "invalid local update ID")
		}
	}
	return nil
}

func localMetadata(metadata *pb.LocalMetadata) error {
	if metadata == nil || len(metadata.Manifest) > localChunkSize || len(metadata.Signature) != 128 {
		return status.Error(codes.InvalidArgument, "invalid local update metadata")
	}
	return nil
}

func runLocal(ctx context.Context, input io.Reader, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, "ota-update", append([]string{"image", "local"}, args...)...)
	cmd.Stdin = input
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	output, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("local update failed: %w: %s", err, stderr.String())
	}
	return output, nil
}

func (s *UpdateServer) PrepareLocalImport(ctx context.Context, req *pb.LocalMetadata) (*pb.LocalCandidate, error) {
	if err := localRole("host"); err != nil {
		return nil, err
	}
	if err := localMetadata(req); err != nil {
		return nil, err
	}
	data, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}
	output, err := runLocal(ctx, bytes.NewReader(append(data, '\n')), "prepare")
	if err != nil {
		return nil, err
	}
	var candidate pb.LocalCandidate
	if err := json.Unmarshal(output, &candidate); err != nil {
		return nil, err
	}
	return &candidate, nil
}

func (s *UpdateServer) AcceptLocalImport(ctx context.Context, req *pb.LocalCandidateRequest) (*pb.LocalCandidate, error) {
	if err := localRole("host"); err != nil {
		return nil, err
	}
	if err := localID(req.Id); err != nil {
		return nil, err
	}
	output, err := runLocal(ctx, nil, "accept", "--id", req.Id)
	if err != nil {
		return nil, err
	}
	var candidate pb.LocalCandidate
	if err := json.Unmarshal(output, &candidate); err != nil {
		return nil, err
	}
	return &candidate, nil
}

func (s *UpdateServer) DiscardLocalImport(ctx context.Context, req *pb.LocalCandidateRequest) (*pb.Empty, error) {
	if err := localID(req.Id); err != nil {
		return nil, err
	}
	action := "discard-admission"
	switch os.Getenv("GHAF_LOCAL_UPDATE_ROLE") {
	case "host":
	case "receiver":
		action = "discard-received"
	default:
		return nil, status.Error(codes.PermissionDenied, "local update cleanup is unavailable")
	}
	_, err := runLocal(ctx, nil, action, "--id", req.Id)
	return &pb.Empty{}, err
}

func (s *UpdateServer) ReceiveLocalImport(stream pb.Update_ReceiveLocalImportServer) error {
	if err := localRole("receiver"); err != nil {
		return err
	}
	return s.receiveLocalImport(stream, localReceiveIdle)
}

func (s *UpdateServer) receiveLocalImport(stream pb.Update_ReceiveLocalImportServer, idle time.Duration) error {
	ctx, cancel := context.WithCancel(stream.Context())
	defer cancel()
	timer := time.AfterFunc(idle, cancel)
	defer timer.Stop()
	type receiveResult struct {
		chunk *pb.LocalReceiveRequest
		err   error
	}
	messages := make(chan receiveResult)
	go func() {
		for {
			chunk, err := stream.Recv()
			select {
			case messages <- receiveResult{chunk, err}:
			case <-ctx.Done():
				return
			}
			if err != nil {
				return
			}
		}
	}()
	recv := func() (*pb.LocalReceiveRequest, error) {
		select {
		case message := <-messages:
			return message.chunk, message.err
		case <-ctx.Done():
			if stream.Context().Err() != nil {
				return nil, status.FromContextError(stream.Context().Err()).Err()
			}
			return nil, status.Error(codes.DeadlineExceeded, "local upload idle timeout")
		}
	}
	first, err := recv()
	if err != nil {
		return err
	}
	if err := localID(first.Id); err != nil {
		return err
	}
	if err := localMetadata(first.Metadata); err != nil {
		return err
	}
	if len(first.Data) != 0 || first.Finish {
		return status.Error(codes.InvalidArgument, "expected metadata first")
	}
	cmd := exec.CommandContext(ctx, "ota-update", "image", "local", "receive", "--id", first.Id)
	cmd.WaitDelay = 2 * time.Second
	input, err := cmd.StdinPipe()
	if err != nil {
		return err
	}
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Start(); err != nil {
		return err
	}
	waited := false
	successful := false
	defer func() {
		input.Close()
		if !waited {
			cancel()
			_ = cmd.Wait()
		}
		if !successful {
			cleanup, stop := context.WithTimeout(context.Background(), 10*time.Second)
			defer stop()
			_, _ = runLocal(cleanup, nil, "discard-received", "--id", first.Id)
		}
	}()
	receiveError := func(cause error) error {
		_ = input.Close()
		// Joining os/exec's stderr copier precedes reading its buffer.
		cancel()
		waitErr := cmd.Wait()
		waited = true
		return fmt.Errorf("receiving bundle: %w (process: %v): %s", cause, waitErr, stderr.String())
	}
	if err := json.NewEncoder(input).Encode(first.Metadata); err != nil {
		return receiveError(err)
	}
	if err := stream.Send(&pb.LocalImportResponse{Phase: "receiving"}); err != nil {
		return err
	}
	var received uint64
	finished := false
	for {
		chunk, err := recv()
		if errors.Is(err, io.EOF) {
			if !finished {
				return status.Error(codes.InvalidArgument, "upload ended without completion")
			}
			break
		}
		if err != nil {
			return err
		}
		if finished || chunk.Id != "" || chunk.Metadata != nil || len(chunk.Data) > localChunkSize || (chunk.Finish && len(chunk.Data) != 0) {
			return status.Error(codes.InvalidArgument, "invalid local update chunk")
		}
		if chunk.Finish {
			finished = true
			continue
		}
		if len(chunk.Data) == 0 {
			return status.Error(codes.InvalidArgument, "empty local update chunk")
		}
		if _, err := input.Write(chunk.Data); err != nil {
			return receiveError(err)
		}
		timer.Reset(idle)
		received += uint64(len(chunk.Data))
		if err := stream.Send(&pb.LocalImportResponse{Phase: "receiving", Completed: received}); err != nil {
			return err
		}
	}
	if err := input.Close(); err != nil {
		return err
	}
	err = cmd.Wait()
	waited = true
	if err != nil {
		return fmt.Errorf("receiving bundle: %w: %s", err, stderr.String())
	}
	successful = true
	return stream.Send(&pb.LocalImportResponse{Phase: "received", Completed: received})
}

func (s *UpdateServer) InstallCandidate(req *pb.LocalCandidateRequest, stream pb.Update_InstallCandidateServer) error {
	if err := localRole("host"); err != nil {
		return err
	}
	if err := localID(req.Id); err != nil {
		return err
	}
	// Output delivery is bounded and lossy under backpressure; the installer
	// must keep draining and writing its slot even without a progress consumer.
	progress := make(chan string, 16)
	output := &localProgress{events: progress}
	cmd := exec.Command("ota-update", "image", "local", "install", "--id", req.Id)
	cmd.Stdout = output
	cmd.Stderr = output
	done := make(chan error, 1)
	go func() { done <- cmd.Run() }()
	subscribed := true
	send := func(text string) {
		if subscribed {
			subscribed = stream.Send(&pb.ImageInstallResponse{Output: stringPtr(text)}) == nil
		}
	}
	send("Installing verified candidate")
	for {
		select {
		case text := <-progress:
			send(text)
		case err := <-done:
			for len(progress) > 0 {
				send(<-progress)
			}
			if err != nil {
				return fmt.Errorf("installing candidate: %w: %s", err, output.tail())
			}
			if !subscribed {
				return nil
			}
			return stream.Send(&pb.ImageInstallResponse{Finished: true, Output: stringPtr("Installation complete; restart required")})
		}
	}
}

func (s *UpdateServer) CandidateStatus(ctx context.Context, req *pb.LocalCandidateRequest) (*pb.LocalImportResponse, error) {
	if err := localRole("host"); err != nil {
		return nil, err
	}
	if err := localID(req.Id); err != nil {
		return nil, err
	}
	output, err := runLocal(ctx, nil, "status", "--id", req.Id)
	if err != nil {
		return nil, err
	}
	var state pb.LocalImportResponse
	if err := json.Unmarshal(output, &state); err != nil {
		return nil, err
	}
	return &state, nil
}

// localProgress retains only a bounded diagnostic tail. Write never waits for RPC I/O.
type localProgress struct {
	mutex      sync.Mutex
	diagnostic []byte
	events     chan string
}

func (w *localProgress) Write(data []byte) (int, error) {
	w.mutex.Lock()
	defer w.mutex.Unlock()
	const limit = 16 * 1024
	n := len(data)
	if len(data) > limit {
		data = data[len(data)-limit:]
	}
	w.diagnostic = append(w.diagnostic, data...)
	if len(w.diagnostic) > limit {
		w.diagnostic = w.diagnostic[len(w.diagnostic)-limit:]
	}
	select {
	case w.events <- string(data):
	default:
	}
	return n, nil
}
func (w *localProgress) tail() string {
	w.mutex.Lock()
	defer w.mutex.Unlock()
	return string(w.diagnostic)
}
