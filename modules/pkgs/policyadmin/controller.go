// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package policyadmin

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"syscall"

	log "github.com/sirupsen/logrus"
)

var PolicyBaseDir = "/etc/policies"

/* Json Parser */
type JsonParser struct {
	data map[string]interface{}
}

/* backend constructor(private) from raw bytes */
func newJsonParserFromBytes(raw []byte) (*JsonParser, error) {
	p := &JsonParser{
		data: make(map[string]interface{}),
	}

	if err := json.Unmarshal(raw, &p.data); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %w", err)
	}

	return p, nil
}

/* Construct from a JSON string */
func NewJsonParserFromString(jsonStr string) (*JsonParser, error) {
	return newJsonParserFromBytes([]byte(jsonStr))
}

/* Construct from a JSON file */
func NewJsonParserFromFile(path string) (*JsonParser, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read JSON file %q: %w", path, err)
	}
	return newJsonParserFromBytes(raw)
}

/*
 * Walks nested keys: GetField("primary_field", "secondary_field", ...)
 * Returns "" if any key is missing or path is invalid.
 */

func (p *JsonParser) GetField(path ...string) string {
	if len(path) == 0 {
		return ""
	}

	var current interface{} = p.data

	for _, key := range path {
		obj, ok := current.(map[string]interface{})
		if !ok {
			return ""
		}
		val, ok := obj[key]
		if !ok {
			return ""
		}
		current = val
	}

	switch v := current.(type) {
	case string:
		return v
	default:
		return fmt.Sprintf("%v", v)
	}
}

/*
 * PolicyAdminController coordinates policy updates for vm-policies.
 * It handles the incoming policy payload, extracts archive,
 * manages revision state, deployment targets, and concurrency control while
 * applying policy updates.
 */
type PolicyAdminController struct {
	/*
	 * Path to the downloaded policy payload.
	 * After a successful update, this field is cleared.
	 */
	pulledPolicyPath string

	/*
	 * Base directory for temporary files/directories.
	 */
	tempDir string

	/*
	 * File handle used to lock the policy directory. lockPolicies()
	 * opens PolicyBaseDir/.lock and acquires an exclusive flock() on
	 * it.
	 */
	lockFile *os.File

	/*
	 * Parsed configuration that maps policy names to deployment
	 * targets.
	 */
	conf *JsonParser

	/*
	 * Parsed metadata for the current update request. Example fields:
	 * - "name"       → name of policy
	 * - "file"       → name of policy file
	 */
	metadata *JsonParser
}

/* Constructor */
func NewPolicyAdminController(conf string, metaDataStr string, pulledPolicyPath string) (*PolicyAdminController, error) {
	log.Debugf("policy-admin:NewPolicyAdminController()")
	tempconf, err := NewJsonParserFromFile(conf)
	if err != nil {
		return nil, err
	}
	metadata, err := NewJsonParserFromString(metaDataStr)
	if err != nil {
		return nil, err
	}
	self := &PolicyAdminController{}
	self.metadata = metadata
	self.conf = tempconf
	self.tempDir = filepath.Join(PolicyBaseDir, ".temp")
	self.lockFile = nil
	self.pulledPolicyPath = pulledPolicyPath
	return self, nil
}

/* Updates the policies to the targets as per defined in config */
func (self *PolicyAdminController) UpdatePolicies() error {
	log.Debugf("policy-admin:UpdatePolicies()")
	if self.metadata == nil {
		return fmt.Errorf("policy-admin: missing metadata")
	}

	return self.installFile()
}

/* Clean temp files/directories */
func (self *PolicyAdminController) Cleanup() {
	if self.pulledPolicyPath != "" {
		os.Remove(self.pulledPolicyPath)
	}

	if self.lockFile != nil {
		self.lockFile.Close()
	}
}

/* A locking mechanism to protect policy directory during policy update */
func (self *PolicyAdminController) lockPolicies() error {
	lockFilePath := filepath.Join(PolicyBaseDir, ".lock")
	lock, err := os.OpenFile(lockFilePath, os.O_CREATE|os.O_RDWR, 0644)
	if err != nil {
		return err
	}
	self.lockFile = lock
	log.Infof("policy-admin: trying to acquire lock...")
	if err := syscall.Flock(int(lock.Fd()), syscall.LOCK_EX); err != nil {
		return fmt.Errorf("policy-admin: failed to acquire lock. (%v)", err)
	}
	return nil
}

/* Releases lock */
func (self *PolicyAdminController) unlockPolicies() error {
	log.Infof("policy-admin: releasing lock...")
	if err := syscall.Flock(int(self.lockFile.Fd()), syscall.LOCK_UN); err != nil {
		return fmt.Errorf("policy-admin: failed to release lock. (%v)", err)
	}
	self.lockFile.Close()
	self.lockFile = nil
	return nil
}

/*
 * Installs or updates a single policy file based on metadata.
 * It replaces the local policy copy and optionally propagates it to the destination.
 */
func (self *PolicyAdminController) installFile() error {
	/* Retrieve policy name from metadata */
	policyName := self.metadata.GetField("name")
	if policyName == "" {
		return fmt.Errorf("policy-admin: missing required field: 'name'")
	}

	/* Retrieve file name from metadata */
	fileName := self.metadata.GetField("file")
	if fileName == "" {
		return fmt.Errorf("policy-admin: missing required field: 'file'")
	}
	log.Infof("policy-admin:installFile() installing policy '%s', file: '%s'", policyName, fileName)

	/*
	 * Acquire an exclusive lock on /etc/policies
	 * to avoid race conditions during concurrent updates.
	 */
	if err := self.lockPolicies(); err != nil {
		return err
	}
	defer self.unlockPolicies()

	/* Create or ensure the local policy directory exists */
	localPolicyDir := filepath.Join(PolicyBaseDir, policyName)
	if err := os.MkdirAll(localPolicyDir, 0755); err != nil {
		return fmt.Errorf("policy-admin: failed to create policy directory: %v", err)
	}

	/* Final path where the incoming file should be stored locally */
	localPolicyFile := filepath.Join(localPolicyDir, fileName)
	log.Infof("policy-admin:installFile() localPolicyFile: '%s'", localPolicyFile)

	/* Move downloaded file to local store (atomic replacement) */
	if err := os.Rename(self.pulledPolicyPath, localPolicyFile); err != nil {
		return fmt.Errorf("policy-admin: failed to write policy locally: %v", err)
	}
	self.pulledPolicyPath = ""

	/* Determine installation destination if configured */
	policyDest := self.conf.GetField(policyName, "targetDir")
	if policyDest == "" {
		log.Infof("policy-admin: no destination configured for '%s', installation only local", policyName)
		return nil
	}
	log.Infof("policy-admin:installFile() policyDest: '%s'", policyDest)

	/* If destination indicates a directory, append filename */
	if strings.HasSuffix(policyDest, "/") {
		policyDest = filepath.Join(policyDest, fileName)
	}

	/* Copy the file to the defined destination */
	if err := self.copyPolicy(localPolicyFile, policyDest); err != nil {
		return fmt.Errorf("policy-admin: failed to deploy policy file: %v", err)
	}

	log.Infof("policy-admin: installed %s → %s", localPolicyFile, policyDest)
	return nil
}

/*
 * Performs actions to install all the defined policies.
 * It scans the policies directory and installs each
 * policy to its configured destination.
 */
func (self *PolicyAdminController) installAllPolicies() error {
	/* List all policy directories */
	entries, err := os.ReadDir(PolicyBaseDir)
	if err != nil {
		return err
	}

	for _, entry := range entries {
		name := entry.Name()
		/* Skip non-policy dirs */
		if strings.HasPrefix(name, ".") {
			continue
		}

		/* Read the destination configured for this policy */
		policyDest := self.conf.GetField(name, "target")
		if policyDest == "" {
			/* Destination not configured → skip copying */
			log.Infof("policy-admin:installAllPolicies() no destination configured for policy %v, skipping", name)
			continue
		}

		/* Read files inside the policy directory */
		files, err := os.ReadDir(filepath.Join(PolicyBaseDir, name))
		if err != nil {
			return err
		}

		for _, file := range files {
			srcFile := filepath.Join(PolicyBaseDir, name, file.Name())

			/* If policyDest ends with '/', preserve filename */
			destPath := policyDest
			if strings.HasSuffix(policyDest, "/") {
				destPath = filepath.Join(policyDest, file.Name())
			}

			/* Copy individual policy file to destination */
			if err := self.copyPolicy(srcFile, destPath); err != nil {
				return err
			}

			log.Infof("policy-admin:installAllPolicies() installed %s → %s", srcFile, destPath)
		}
	}

	/* No errors → installation completed */
	return nil
}

/*
 * Copies a policy file from src to dest.
 * Ensures destination directory exists before writing.
 */
func (self *PolicyAdminController) copyPolicy(src string, dest string) error {
	/* Ensure the destination directory structure exists */
	log.Infof("policy-admin:copyPolicy() Creating directory structure for %s", filepath.Dir(dest))
	if err := os.MkdirAll(filepath.Dir(dest), 0755); err != nil {
		return fmt.Errorf("policy-admin: failed to create destination directory (%v)", err)
	}

	log.Debugf("policy-admin:copyPolicy() Copying policy from %s to %s", src, dest)

	/* Create the destination file (truncate if exists) */
	dstFile, err := os.OpenFile(dest, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0644)
	if err != nil {
		return fmt.Errorf("policy-admin: failed to open destination file. (%v)", err)
	}
	defer dstFile.Close()

	/* Open the source policy file for reading */
	srcFile, err := os.Open(src)
	if err != nil {
		return fmt.Errorf("policy-admin: failed to open policy file. (%v)", err)
	}
	defer srcFile.Close()

	/* Perform the file copy operation */
	if _, err = io.Copy(dstFile, srcFile); err != nil {
		return fmt.Errorf("policy-admin: failed to copy policy. (%v)", err)
	}

	return nil
}

/* PathExists returns true if the given filesystem path exists. */
func PathExists(path string) bool {
	_, err := os.Stat(path)
	if err == nil {
		return true
	}
	return !os.IsNotExist(err)
}
