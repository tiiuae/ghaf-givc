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
	"time"

	pb "givc/modules/api/update"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

const localChunkSize = 256 * 1024

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
	first, err := stream.Recv()
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
	ctx, cancel := context.WithCancel(stream.Context())
	defer cancel()
	cmd := exec.CommandContext(ctx, "ota-update", "image", "local", "receive", "--id", first.Id)
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
	if err := json.NewEncoder(input).Encode(first.Metadata); err != nil {
		return err
	}
	if err := stream.Send(&pb.LocalImportResponse{Phase: "receiving"}); err != nil {
		return err
	}
	var received uint64
	finished := false
	for {
		chunk, err := stream.Recv()
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
			return fmt.Errorf("receiving bundle: %w", err)
		}
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
	// Keep draining the process after a subscriber disconnects; closing its pipes can abort a slot write.
	_ = stream.Send(&pb.ImageInstallResponse{Output: stringPtr("Installing verified candidate")})
	cmd := exec.Command("ota-update", "image", "local", "install", "--id", req.Id)
	var stderr bytes.Buffer
	cmd.Stdout = io.Discard
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("installing candidate: %w: %s", err, stderr.String())
	}
	return stream.Send(&pb.ImageInstallResponse{Finished: true, Output: stringPtr("Installation complete; restart required")})
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
