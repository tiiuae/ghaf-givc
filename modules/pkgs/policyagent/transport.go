// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package policyagent

import (
	"archive/tar"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	pb "givc/modules/api/policyagent"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type ActionMap map[string]string

type PolicyAgentServer struct {
	pb.UnimplementedPolicyAgentServer
}

func (s *PolicyAgentServer) Name() string {
	return "Policy Agent Server"
}

func (s *PolicyAgentServer) RegisterGrpcService(srv *grpc.Server) {
	pb.RegisterPolicyAgentServer(srv, s)
}

func NewPolicyAgentServer() (*PolicyAgentServer, error) {
	return &PolicyAgentServer{}, nil
}

func (s *PolicyAgentServer) StreamPolicy(stream pb.PolicyAgent_StreamPolicyServer) error {
	log.SetLevel(log.DebugLevel)
	policyBaseDir := "/etc/policies"
	actionFile := "/etc/policy-agent/config.json"

	log.Debugf("policy-agent: policy stream initiated by givc-admin.")

	tempFile, err := os.CreateTemp("", "policy-*.tar.gz")
	if err != nil {
		log.Errorf("policy-agent: failed to create temporary file: %v", err)
		return err
	}
	defer os.Remove(tempFile.Name())

	changeSet := ""
	oldRev := ""
	newRev := ""

	for {
		in, err := stream.Recv()
		if err == io.EOF {
			log.Infof("policy-agent: received message from givc-admin.")
			break
		}

		if err != nil {
			log.Errorf("policy-agent: error receiving from policy stream: %v", err)
			return stream.SendAndClose(&pb.Status{Status: "FAILED"})
		}

		archive_chunk := in.GetArchiveChunk()
		if archive_chunk != nil {
			log.Debugf("policy-agent: writing chunk of %d bytes to temporary file....", len(archive_chunk))
			if _, err := tempFile.Write(archive_chunk); err != nil {
				log.Errorf("policy-agent: failed to write to temporary file: %v", err)
				return stream.SendAndClose(&pb.Status{Status: "FAILED"})
			}
		}

		if val := in.GetChangeSet(); val != "" {
			changeSet = val
		}

		if val := in.GetOldRev(); val != "" {
			oldRev = val
		}

		if val := in.GetNewRev(); val != "" {
			newRev = val
		}
	}

	if newRev == "" {
		log.Errorf("policy-agent: spurious policy received")
		return stream.SendAndClose(&pb.Status{Status: "FAILED"})
	}

	log.Debugf("policy-agent: policy update info: ChangeSet=%s OldRev=%s NewRev=%s", changeSet, oldRev, newRev)

	tempFile.Close()

	vmPolicyDir := filepath.Join(policyBaseDir, "vm-policies")
	revFile := filepath.Join(policyBaseDir, ".rev")

	/* Extract only if the policy revision is changed */
	extractPolicy := false
	if GetFileSize(tempFile.Name()) > 0 {
		if FileExists(revFile) {
			sha, _ := os.ReadFile(revFile)
			if string(sha) != newRev {
				extractPolicy = true
			}
		} else {
			extractPolicy = true
		}
	}

	log.Debugf("policy-agent: ExtractiPolicy?=%v", extractPolicy)
	if extractPolicy {
		/* Uncompress policy tar ball */
		log.Debugf("policy-agent: extracting policy archive %s to %s", tempFile.Name(), vmPolicyDir)
		if err := extractTarGz(tempFile.Name(), vmPolicyDir); err != nil {
			log.Errorf("policy-agent: failed to extract policy archive: %v", err)
			return stream.SendAndClose(&pb.Status{Status: "FAILED"})
		}

		/* Update revision file */
		err := os.WriteFile(revFile, []byte(newRev), 0664)
		if err != nil {
			log.Errorf("policy-agent: failed to write to sha file: %v", err)
			return stream.SendAndClose(&pb.Status{Status: "FAILED"})
		}

		/* Install only updated policies */
		if !FileExists(actionFile) {
			log.Infof("policy-agent: policy update ignored, policy install rules not found.")
			return stream.SendAndClose(&pb.Status{Status: "OK"})
		}

		installRules, err := LoadActionMap(actionFile)
		if err != nil {
			log.Errorf("policy-agent: error loading install rules: %v", err)
		}

		if err := ProcessChangeset(changeSet, vmPolicyDir, installRules); err != nil {
			log.Errorf("policy-agent: error processing changeset: %v", err)
		}

	}

	log.Infof("policy-agent: successfully extracted policies.")
	return stream.SendAndClose(&pb.Status{Status: "OK"})
}

/* Extracts a .tar.gz file to a destination directory. */
func extractTarGz(tarGzPath string, destDir string) error {
	/* Clean destination directory */
	if err := os.RemoveAll(destDir); err != nil {
		return fmt.Errorf("policy-agent: cleaning destination directory, error: %w", err)
	}

	/* Create destination directory */

	file, err := os.Open(tarGzPath)
	if err != nil {
		return err
	}
	defer file.Close()

	gzr, err := gzip.NewReader(file)
	if err != nil {
		return err
	}
	defer gzr.Close()

	tr := tar.NewReader(gzr)
	/* Extract individual components from tar ball */
	for {
		header, err := tr.Next()
		if err == io.EOF {
			break
		}

		if err != nil {
			return err
		}

		target := filepath.Join(destDir, header.Name)
		/* check for valid path */
		if !strings.Contains(target, "..") {
			switch header.Typeflag {
			case tar.TypeDir:
				log.Debugf("policy-agent: creating directory: %s", target)
				if err := os.MkdirAll(target, 0775); err != nil {
					return err
				}

			case tar.TypeReg:
				log.Debugf("policy-agent: extracting File: %s", target)
				if err := os.MkdirAll(filepath.Dir(target), 0775); err != nil {
					return err
				}

				outFile, err := os.OpenFile(target, os.O_CREATE|os.O_RDWR|os.O_TRUNC, 0775)
				if err != nil {
					return err
				}

				defer outFile.Close()
				if _, err := io.Copy(outFile, tr); err != nil {
					return err
				}
			}
		} else {
			log.Warnf("policy-agent: invalid file path in tar archive: %s", target)
		}
	}
	return nil
}

/* Loads install actions for the policies */
func LoadActionMap(jsonPath string) (ActionMap, error) {
	data, err := os.ReadFile(jsonPath)
	if err != nil {
		return nil, fmt.Errorf("reading action json: %w", err)
	}
	var m ActionMap
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("unmarshal action json: %w", err)
	}
	return m, nil
}

/* Performs defined actions for each modified policies */
func ProcessChangeset(changeset, policyDir string, actions ActionMap) error {
	/*
	 * No changeset is available them perform action
	 * against all policies in the archive.
	 */
	trimmed := strings.TrimSpace(changeset)

	if trimmed == "" {
		return installAllPolicies(policyDir, actions)
	}

	/* Parse the change set to get the name of the modified policies */
	names := getModifiedPolicies(changeset, "vm-policies")
	if len(names) == 0 {
		return nil
	}

	for name := range names {
		action, ok := actions[name]
		if !ok {
			log.Infof("policy-agent: no action found for %q, skipping\n", name)
			continue
		}

		targetPath := filepath.Join(policyDir, name)
		if err := installPolicy(action, targetPath); err != nil {
			return fmt.Errorf("running action for %q: %w", name, err)
		}
	}

	return nil
}

/* Gets the name of modified policies from the changeset */
func getModifiedPolicies(changeset, root string) map[string]struct{} {
	result := make(map[string]struct{})
	lines := strings.Split(changeset, "\n")

	prefix := root + "/"

	/*
	 * From each line of changeset extract entries in vm-policies.
	 * We expect each file/directory inside vm-policies is a individual policy.
	 */
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}

		parts := strings.Fields(line)
		if len(parts) < 2 {
			continue
		}
		path := parts[1]

		if !strings.HasPrefix(path, prefix) {
			continue
		}

		/* Strip "vm-policies/" */
		rel := strings.TrimPrefix(path, prefix)
		if rel == "" {
			continue
		}

		/*
		 * Take only the first path component: no recursion.
		 */
		top := strings.SplitN(rel, "/", 2)[0]
		if top != "" {
			result[top] = struct{}{}
		}
	}

	return result
}

/* Performs actions to install all the defined policies */
func installAllPolicies(policyDir string, actions ActionMap) error {
	/*
	 * Check for each entry in action map json if a policy file/dir exists
	 * perform the action.
	 */
	for name, action := range actions {
		targetPath := filepath.Join(policyDir, name)
		if _, err := os.Stat(targetPath); err != nil {
			if os.IsNotExist(err) {
				continue
			}
			return fmt.Errorf("stat %q: %w", targetPath, err)
		}

		if err := installPolicy(action, targetPath); err != nil {
			return fmt.Errorf("running action for %q: %w", name, err)
		}
	}
	return nil
}

/* Runs action to install the policy available at targetPath */
func installPolicy(action, targetPath string) error {
	action = strings.TrimSpace(action)

	if action == "" {
		return fmt.Errorf("empty action command")
	}

	/* Before running replace {target} with policy path in the action command */
	action = strings.ReplaceAll(action, "{target}", targetPath)
	parts := strings.Fields(action)
	cmdName := parts[0]
	if cmdName == "" {
		return fmt.Errorf("empty command name")
	}

	args := parts[1:]

	cmd := exec.Command(cmdName, args...)
	log.Infof("policy-agent: executing policy install command: %s %s", cmdName, strings.Join(args, " "))
	cmd.Run()
	cmd.Wait()
	log.Infof("policy-agent: policy install command completed with exit code %d", cmd.ProcessState.ExitCode())
	if cmd.ProcessState.ExitCode() != 0 {
		return fmt.Errorf("command exited with code %d", cmd.ProcessState.ExitCode())
	}

	return nil
}

/* Checks if a file exists */
func FileExists(path string) bool {
	_, err := os.Stat(path)
	if err == nil {
		return true
	}
	return !os.IsNotExist(err)
}

/* Gets the size of a file */
func GetFileSize(path string) int64 {
	info, err := os.Stat(path)
	if err != nil {
		return 0
	}
	return info.Size()
}
