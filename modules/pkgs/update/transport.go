// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package update

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os/exec"
	"strings"
	"sync"

	pbadmin "givc/modules/api/admin"
	pbupdate "givc/modules/api/update"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
	grpc_codes "google.golang.org/grpc/codes"
	grpc_status "google.golang.org/grpc/status"
)

type process struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	stdout io.ReadCloser
	stderr io.ReadCloser
}

type UpdateServer struct {
	pbupdate.UnimplementedUpdateServer
}

type generationDetails struct {
	Generation            int32   `json:"generation"`
	NixosVersion          string  `json:"nixosVersion"`
	KernelVersion         string  `json:"kernelVersion"`
	ConfigurationRevision *string `json:"configurationRevision"`
	StorePath             string  `json:"storePath"`
	Current               bool    `json:"current"`
}

type discoverUpdate struct {
	Repository string `json:"repository"`
	Tag        string `json:"tag"`
	Version    string `json:"version"`
	Hash       string `json:"hash"`
}

type registryEvent struct {
	Event       string  `json:"event"`
	Reference   string  `json:"reference,omitempty"`
	Destination string  `json:"destination,omitempty"`
	Digest      string  `json:"digest,omitempty"`
	Downloaded  uint64  `json:"downloaded,omitempty"`
	Total       *uint64 `json:"total,omitempty"`
	Path        string  `json:"path,omitempty"`
	Stage       string  `json:"stage,omitempty"`
}

func (s *UpdateServer) Name() string {
	return "Update Server"
}

func (s *UpdateServer) RegisterGrpcService(srv *grpc.Server) {
	pbupdate.RegisterUpdateServer(srv, s)
}

func NewUpdateServer() (*UpdateServer, error) {
	updateServer := UpdateServer{}
	return &updateServer, nil
}

func (s *UpdateServer) ListGenerations(_ context.Context, _ *pbadmin.Empty) (*pbadmin.ListGenerationsResponse, error) {
	stdout, stderr, rc, err := runUpdateCommand("get")
	if err != nil {
		return nil, err
	}
	if rc != 0 {
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("ota-update get failed: %s", strings.TrimSpace(string(stderr))))
	}

	var generations []generationDetails
	if err := json.Unmarshal(stdout, &generations); err != nil {
		return nil, fmt.Errorf("failed to parse ota-update output: %w", err)
	}

	resp := &pbadmin.ListGenerationsResponse{List: make([]*pbadmin.Generation, 0, len(generations))}
	for _, item := range generations {
		resp.List = append(resp.List, &pbadmin.Generation{
			Generation:            item.Generation,
			Date:                  "bogus",
			NixosVersion:          item.NixosVersion,
			KernelVersion:         item.KernelVersion,
			ConfigurationRevision: valueOrDefault(item.ConfigurationRevision, "unknown"),
			Specialisations:       []string{},
			Current:               item.Current,
			StorePath:             item.StorePath,
		})
	}
	return resp, nil
}

func (s *UpdateServer) Discover(_ context.Context, request *pbadmin.RegistryDiscoverRequest) (*pbadmin.RegistryDiscoverResponse, error) {
	args := append([]string{"registry", "--output", "jsonl"}, registryArgs(request)...)
	args = append(args, "discover", request.Reference)
	stdout, stderr, rc, err := runUpdateCommand(args...)
	if err != nil {
		return nil, err
	}
	if rc != 0 {
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("ota-update registry discover failed: %s", strings.TrimSpace(string(stderr))))
	}

	output := afterDoneOutput(stdout)
	var updates []discoverUpdate
	if err := json.Unmarshal([]byte(output), &updates); err != nil {
		return nil, fmt.Errorf("failed to parse discover output: %w", err)
	}

	resp := &pbadmin.RegistryDiscoverResponse{List: make([]*pbadmin.AvailableUpdate, 0, len(updates))}
	for _, item := range updates {
		resp.List = append(resp.List, &pbadmin.AvailableUpdate{
			Repository: item.Repository,
			Tag:        item.Tag,
			Version:    item.Version,
			Hash:       item.Hash,
		})
	}
	return resp, nil
}

func (s *UpdateServer) Changelog(_ context.Context, request *pbadmin.RegistryChangelogRequest) (*pbadmin.RegistryChangelogResponse, error) {
	args := append([]string{"registry", "--output", "jsonl"}, registryArgs(request)...)
	args = append(args, "changelog", request.Reference)
	stdout, stderr, rc, err := runUpdateCommand(args...)
	if err != nil {
		return nil, err
	}
	if rc != 0 {
		return nil, grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("ota-update registry changelog failed: %s", strings.TrimSpace(string(stderr))))
	}

	return &pbadmin.RegistryChangelogResponse{Changelog: afterDoneOutput(stdout)}, nil
}

func (s *UpdateServer) Pull(request *pbadmin.RegistryPullRequest, stream pbupdate.Update_PullServer) error {
	args := append([]string{"registry", "--output", "jsonl"}, registryArgs(request)...)
	args = append(args, "pull", request.Reference, "--destination", request.Destination)
	args = append(args, "--validate")

	return runPullCommand(stream, args...)
}

func (s *UpdateServer) ImageInstall(request *pbadmin.ImageInstallRequest, stream pbupdate.Update_ImageInstallServer) error {
	args := []string{"image", "install", "--manifest", request.Manifest}
	args = append(args, "--validate")

	return runOutputCommand(
		stream,
		args,
		func(stdout []byte, eof bool) pbadmin.ImageInstallResponse {
			return pbadmin.ImageInstallResponse{
				Finished: eof,
				Output:   stringPtr(string(stdout)),
			}
		},
		func(stderr []byte, eof bool) pbadmin.ImageInstallResponse {
			return pbadmin.ImageInstallResponse{
				Finished: eof,
				Error:    stringPtr(string(stderr)),
			}
		},
	)
}

func (s *UpdateServer) InstallCachix(request *pbadmin.Cachix, stream pbupdate.Update_InstallCachixServer) error {
	args := []string{"cachix", request.Pin, "--cache", request.Cache}
	if request.Token != nil {
		args = append(args, "--token", *request.Token)
	}
	if request.CachixHost != nil {
		args = append(args, "--cachix-host", *request.CachixHost)
	}

	return runOutputCommand(
		stream,
		args,
		func(stdout []byte, eof bool) pbadmin.SetGenerationResponse {
			return pbadmin.SetGenerationResponse{
				Finished: eof,
				Output:   stringPtr(string(stdout)),
			}
		},
		func(stderr []byte, eof bool) pbadmin.SetGenerationResponse {
			return pbadmin.SetGenerationResponse{
				Finished: eof,
				Error:    stringPtr(string(stderr)),
			}
		},
	)
}

func registryArgs(request interface{}) []string {
	args := []string{}
	if r, ok := request.(interface{ GetInsecure() bool }); ok && r.GetInsecure() {
		args = append(args, "--insecure")
	}
	switch req := request.(type) {
	case *pbadmin.RegistryDiscoverRequest:
		args = append(args, registryCredentials(req.Credentials)...)
	case *pbadmin.RegistryChangelogRequest:
		args = append(args, registryCredentials(req.Credentials)...)
	case *pbadmin.RegistryPullRequest:
		args = append(args, registryCredentials(req.Credentials)...)
	}
	return args
}

func registryCredentials(credentials *pbadmin.RegistryCredentials) []string {
	if credentials == nil || credentials.Auth == nil {
		return nil
	}
	switch auth := credentials.Auth.(type) {
	case *pbadmin.RegistryCredentials_Basic:
		return []string{"--username", auth.Basic.Username, "--password", auth.Basic.Password}
	case *pbadmin.RegistryCredentials_Bearer:
		return []string{"--token", auth.Bearer.Token}
	default:
		return nil
	}
}

func runUpdateCommand(args ...string) ([]byte, []byte, int, error) {
	cmd := exec.Command("ota-update", args...)
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return stdout.Bytes(), stderr.Bytes(), exitErr.ExitCode(), nil
		}
		return nil, nil, -1, err
	}
	return stdout.Bytes(), stderr.Bytes(), 0, nil
}

func runPullCommand(stream pbupdate.Update_PullServer, args ...string) error {
	cmd := exec.Command("ota-update", args...)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return err
	}
	if err := cmd.Start(); err != nil {
		return err
	}

	var wg sync.WaitGroup
	wg.Add(2)

	go func() {
		defer wg.Done()
		readPullProgress(stdout, stream)
	}()
	go func() {
		defer wg.Done()
		logOtaStderr(stderr)
	}()

	err = cmd.Wait()
	wg.Wait()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("ota-update registry pull failed with rc %d", exitErr.ExitCode()))
		}
		return err
	}

	return nil
}

func runOutputCommand[T any](stream grpc.ServerStreamingServer[T], args []string, makeStdout func([]byte, bool) T, makeStderr func([]byte, bool) T) error {
	cmd := exec.Command("ota-update", args...)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return err
	}
	if err := cmd.Start(); err != nil {
		return err
	}

	var wg sync.WaitGroup
	wg.Add(2)

	go streamOutput(stdout, stream, &wg, makeStdout)
	go streamOutput(stderr, stream, &wg, makeStderr)

	err = cmd.Wait()
	wg.Wait()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return grpc_status.Error(grpc_codes.Unknown, fmt.Sprintf("ota-update failed with rc %d", exitErr.ExitCode()))
		}
		return err
	}
	return nil
}

func streamOutput[T any](reader io.ReadCloser, stream grpc.ServerStreamingServer[T], wg *sync.WaitGroup, makeResponse func(data []byte, eof bool) T) {
	defer reader.Close()
	defer wg.Done()
	bufReader := bufio.NewReader(reader)
	buffer := make([]byte, 1024)
	for {
		n, err := bufReader.Read(buffer)
		if n > 0 {
			resp := makeResponse(buffer[:n], false)
			if sendErr := stream.Send(&resp); sendErr != nil {
				log.Errorf("failed to stream: %v", sendErr)
				return
			}
		}
		if err == io.EOF {
			resp := makeResponse([]byte{}, true)
			if sendErr := stream.Send(&resp); sendErr != nil {
				log.Warnf("failed to stream eof: %v", sendErr)
			}
			break
		}
		if err != nil {
			log.Errorf("unknown error reading input: %v", err)
			return
		}
	}
}

func readPullProgress(reader io.ReadCloser, stream pbupdate.Update_PullServer) {
	defer reader.Close()
	bufReader := bufio.NewReader(reader)
	var stdoutBuf []byte
	seenDone := false
	var resultBuf strings.Builder

	flushLine := func(line []byte) error {
		if len(line) == 0 {
			return nil
		}
		var event registryEvent
		if err := json.Unmarshal(line, &event); err == nil && event.Event != "" {
			if event.Event == "done" {
				seenDone = true
			}
			if resp := registryEventToProgress(event); resp != nil {
				if err := stream.Send(&pbadmin.RegistryPullResponse{
					Update: &pbadmin.RegistryPullResponse_Progress{Progress: resp},
				}); err != nil {
					return err
				}
			}
			return nil
		}

		if seenDone {
			if resultBuf.Len() > 0 {
				resultBuf.WriteByte('\n')
			}
			resultBuf.Write(line)
		}
		return nil
	}

	for {
		line, err := bufReader.ReadBytes('\n')
		if len(line) > 0 {
			stdoutBuf = append(stdoutBuf, line...)
			for {
				newline := bytes.IndexByte(stdoutBuf, '\n')
				if newline < 0 {
					break
				}
				current := append([]byte(nil), stdoutBuf[:newline]...)
				if err := flushLine(bytes.TrimSpace(current)); err != nil {
					log.Warnf("failed to send pull progress: %v", err)
					return
				}
				stdoutBuf = stdoutBuf[newline+1:]
			}
		}
		if err == io.EOF {
			if len(stdoutBuf) > 0 {
				if err := flushLine(bytes.TrimSpace(stdoutBuf)); err != nil {
					log.Warnf("failed to send pull progress: %v", err)
				}
			}
			break
		}
		if err != nil {
			log.Warnf("read pull progress failed: %v", err)
			return
		}
	}

	if result := parsePullResult(resultBuf.String()); result != nil {
		if err := stream.Send(&pbadmin.RegistryPullResponse{
			Update: &pbadmin.RegistryPullResponse_Result{Result: result},
		}); err != nil {
			log.Warnf("failed to send pull result: %v", err)
		}
	}
}

func logOtaStderr(reader io.ReadCloser) {
	defer reader.Close()
	bufReader := bufio.NewReader(reader)
	buffer := make([]byte, 1024)
	for {
		n, err := bufReader.Read(buffer)
		if err == io.EOF {
			return
		}
		if err != nil {
			log.Warnf("stderr read failed: %v", err)
			return
		}
		log.Debugf("ota-update stderr: %s", strings.TrimSpace(string(buffer[:n])))
	}
}

func registryEventToProgress(event registryEvent) *pbadmin.RegistryPullProgress {
	switch event.Event {
	case "pull_started":
		return &pbadmin.RegistryPullProgress{
			Event: &pbadmin.RegistryPullProgress_PullStarted{
				PullStarted: &pbadmin.RegistryPullStarted{
					Reference:   event.Reference,
					Destination: event.Destination,
				},
			},
		}
	case "blob_downloading":
		progress := &pbadmin.RegistryPullProgress{
			Event: &pbadmin.RegistryPullProgress_BlobDownloading{
				BlobDownloading: &pbadmin.RegistryBlobDownloading{
					Digest:     event.Digest,
					Downloaded: event.Downloaded,
				},
			},
		}
		if event.Total != nil {
			progress.GetBlobDownloading().Total = event.Total
		}
		return progress
	case "blob_verified":
		return &pbadmin.RegistryPullProgress{
			Event: &pbadmin.RegistryPullProgress_BlobVerified{
				BlobVerified: event.Digest,
			},
		}
	case "manifest_written":
		return &pbadmin.RegistryPullProgress{
			Event: &pbadmin.RegistryPullProgress_ManifestWritten{
				ManifestWritten: event.Path,
			},
		}
	case "cancelled":
		return &pbadmin.RegistryPullProgress{
			Event: &pbadmin.RegistryPullProgress_Cancelled{
				Cancelled: event.Stage,
			},
		}
	case "done":
		return &pbadmin.RegistryPullProgress{
			Event: &pbadmin.RegistryPullProgress_Done{
				Done: true,
			},
		}
	default:
		return nil
	}
}

func parsePullResult(stdout string) *pbadmin.RegistryPullResult {
	var outputDir string
	var manifestPath string
	for _, line := range strings.Split(stdout, "\n") {
		if value, ok := strings.CutPrefix(line, "pulled to: "); ok {
			outputDir = value
		}
		if value, ok := strings.CutPrefix(line, "manifest: "); ok {
			manifestPath = value
		}
	}
	if outputDir == "" || manifestPath == "" {
		return nil
	}
	return &pbadmin.RegistryPullResult{
		OutputDir:    outputDir,
		ManifestPath: manifestPath,
	}
}

func afterDoneOutput(stdout []byte) string {
	seenDone := false
	var output strings.Builder
	for _, rawLine := range bytes.Split(stdout, []byte{'\n'}) {
		line := bytes.TrimSpace(rawLine)
		if len(line) == 0 {
			continue
		}
		var event registryEvent
		if err := json.Unmarshal(line, &event); err == nil && event.Event != "" {
			if event.Event == "done" {
				seenDone = true
			}
			continue
		}
		if seenDone {
			if output.Len() > 0 {
				output.WriteByte('\n')
			}
			output.Write(line)
		}
	}
	return output.String()
}

func valueOrDefault(value *string, fallback string) string {
	if value == nil || *value == "" {
		return fallback
	}
	return *value
}

func stringPtr(value string) *string {
	if value == "" {
		return nil
	}
	return &value
}
