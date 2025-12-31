// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package policyadmin

import (
	"archive/tar"
	"compress/gzip"
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
	 * - For "file" updates: this is a single policy file in plain text.
	 * - For "dir" updates: this is a .tar.gz archive.
	 * After a successful update, this field is cleared.
	 */
	pulledPolicyPath string

	/*
	 * Path to the temporary directory where a policy archive is
	 * decompressed.
	 */
	uncompressedPolicyPath string

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
	 * - "type"       → "file" or "dir"
	 * - "rev"        → revision of the policy set (used in "dir" type only)
	 * - "base"       → base revision for comparison (used in "dir" type only)
	 * - "changeset"  → list of modified policies for incremental
	 *                  updates (used in "dir" type only)
	 * - "name"       → name of policy (used in "file" type only)
	 * - "file"       → name of policy file (used in "file" type only)
	 */
	metadata *JsonParser
}

/* Constructor */
func NewPolicyAdminController(conf string, metaDataStr string, pulledPolicyPath string) (*PolicyAdminController, error) {
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
	if self.metadata == nil {
		return fmt.Errorf("policy-admin: missing metadata")
	}

	datatype := self.metadata.GetField("type")
	if datatype == "file" {
		/* Single policy file */
		return self.installFile()
	} else if datatype == "dir" {
		/* Policy archive */
		return self.installArchive()
	}

	return fmt.Errorf("policy-admin: unknown datatype: %s", datatype)
}

/* Clean temp files/directories */
func (self *PolicyAdminController) Cleanup() {
	if self.pulledPolicyPath != "" {
		os.Remove(self.pulledPolicyPath)
	}

	if self.uncompressedPolicyPath != "" {
		os.RemoveAll(self.uncompressedPolicyPath)
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

	/*
	 * Acquire an exclusive lock on /etc/policies
	 * to avoid race conditions during concurrent updates.
	 */
	if err := self.lockPolicies(); err != nil {
		return err
	}
	defer self.unlockPolicies()

	/* Create or ensure the local policy directory exists */
	localPolicyDir := filepath.Join(PolicyBaseDir, "vm-policies", policyName)
	if err := os.MkdirAll(localPolicyDir, 0755); err != nil {
		return fmt.Errorf("policy-admin: failed to create policy directory: %v", err)
	}

	/* Final path where the incoming file should be stored locally */
	localPolicyFile := filepath.Join(localPolicyDir, fileName)

	/* Move downloaded file to local store (atomic replacement) */
	if err := os.Rename(self.pulledPolicyPath, localPolicyFile); err != nil {
		return fmt.Errorf("policy-admin: failed to write policy locally: %v", err)
	}
	self.pulledPolicyPath = ""

	/* Determine installation destination if configured */
	policyDest := self.conf.GetField(policyName, "dest")
	if policyDest == "" {
		log.Infof("policy-admin: no destination configured for '%s', installation only local", policyName)
		return nil
	}

	/* If destination indicates a directory, append filename */
	if strings.HasSuffix(policyDest, "/") {
		policyDest = filepath.Join(policyDest, fileName)
	}

	/* Copy the file to the defined destination */
	if err := self.copyPolicy(localPolicyFile, policyDest); err != nil {
		return fmt.Errorf("policy-admin: failed to deploy policy file: %v", err)
	}

	log.Infof("policy-admin: installed %s → %s", fileName, policyDest)
	return nil
}

/*
 * Installs a new policy archive if its revision differs
 * from the currently applied one. Validates revision, extracts archive,
 * processes changes, and updates the revision marker file.
 */
func (self *PolicyAdminController) installArchive() error {
	/* Retrieve policy revision */
	newRev := self.metadata.GetField("rev")
	if newRev == "" {
		return fmt.Errorf("policy-admin: missing required field: 'rev'")
	}
	revFile := filepath.Join(PolicyBaseDir, ".rev")

	/* Determine if uncompression is required (revision changed or first install) */
	extractPolicy := false
	var localrev string
	if PathExists(revFile) {
		localrevBytes, _ := os.ReadFile(revFile)
		localrev = string(localrevBytes)
		if localrev != newRev {
			extractPolicy = true
		}
	} else {
		localrev = ""
		extractPolicy = true
	}

	if extractPolicy {
		/* Uncompress policy archive (.tar.gz) to temporary directory */
		if err := self.decompressPolicy(); err != nil {
			log.Errorf("policy-admin: failed to decompress policy: %v", err)
			return err
		}

		/*
		 * Acquire an exclusive lock on /etc/policies
		 * to avoid race conditions during concurrent updates.
		 */
		if err := self.lockPolicies(); err != nil {
			return err
		}
		defer self.unlockPolicies()

		/* Apply changes based on changeset and local revision */
		if err := self.processChangeset(localrev); err != nil {
			log.Errorf("policy-admin: error processing changeset: %v", err)
			return err
		}

		/* Update revision marker to reflect new state */
		if err := os.WriteFile(revFile, []byte(newRev), 0664); err != nil {
			log.Errorf("policy-admin: failed to update revision file: %v", err)
			return err
		}
	}

	return nil
}

/*
 * Applies modifications to local policies based on the received changeset.
 * If the changeset is empty or inconsistent, a full reinstall is performed.
 */
func (self *PolicyAdminController) processChangeset(localrev string) error {
	/* Remove currently installed policies */
	localPolicyDir := filepath.Join(PolicyBaseDir, "vm-policies")
	if err := os.RemoveAll(localPolicyDir); err != nil {
		return err
	}

	/* Replace with the newly extracted policies */
	if err := os.Rename(self.uncompressedPolicyPath, localPolicyDir); err != nil {
		return err
	}
	self.uncompressedPolicyPath = ""

	/*
	 * Install all policies if ANY of the following is true:
	 * 1) No changeset is provided (fresh clone or clean install)
	 * 2) Base revision does not match last applied revision (desynchronization)
	 */
	changeset := strings.TrimSpace(self.metadata.GetField("changeset"))
	baseRev := self.metadata.GetField("base")
	if changeset == "" || baseRev != localrev {
		return self.installAllPolicies()
	}

	/* Extract policy names that were modified (incremental update) */
	names := self.getModifiedPolicies(changeset, "vm-policies")
	if len(names) == 0 {
		return nil /* Nothing to do */
	}

	/* Apply changes to modified policies */
	for name := range names {
		targetPath := filepath.Join(localPolicyDir, name)

		/* Read updated policy files for this specific policy */
		files, err := os.ReadDir(targetPath)
		if err != nil {
			return err
		}

		/* Destination where this policy should be installed */
		policyDest := self.conf.GetField(name, "dest")
		if policyDest == "" {
			log.Infof("policy-admin: no destination configured for '%v', skipping", name)
			continue
		}

		/* Copy updated files to destination */
		for _, file := range files {
			dest := policyDest
			/* If destination indicates directory, append filename */
			if strings.HasSuffix(policyDest, "/") {
				dest = filepath.Join(policyDest, file.Name())
			}

			src := filepath.Join(targetPath, file.Name())
			if err := self.copyPolicy(src, dest); err != nil {
				return err
			}

			log.Debugf("policy-admin: updated policy '%s' → %s", name, dest)
		}
	}

	return nil
}

/* Gets the name of modified policies from the changeset */
func (self *PolicyAdminController) getModifiedPolicies(changeset, root string) map[string]struct{} {
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

/*
 * Performs actions to install all the defined policies.
 * It scans the vm-policies directory and installs each
 * policy to its configured destination.
 */
func (self *PolicyAdminController) installAllPolicies() error {
	/* Base directory where extracted policies are located */
	targetPath := filepath.Join(PolicyBaseDir, "vm-policies")

	/* List all policy directories */
	entries, err := os.ReadDir(targetPath)
	if err != nil {
		return err
	}

	for _, entry := range entries {
		name := entry.Name()

		/* Read the destination configured for this policy */
		policyDest := self.conf.GetField(name, "dest")
		if policyDest == "" {
			/* Destination not configured → skip copying */
			log.Infof("policy-admin: no destination configured for policy %v, skipping", name)
			continue
		}

		/* Read files inside the policy directory */
		files, err := os.ReadDir(filepath.Join(targetPath, name))
		if err != nil {
			return err
		}

		for _, file := range files {
			srcFile := filepath.Join(targetPath, name, file.Name())

			/* If policyDest ends with '/', preserve filename */
			destPath := policyDest
			if strings.HasSuffix(policyDest, "/") {
				destPath = filepath.Join(policyDest, file.Name())
			}

			/* Copy individual policy file to destination */
			if err := self.copyPolicy(srcFile, destPath); err != nil {
				return err
			}

			log.Debugf("policy-admin: installed %s → %s", srcFile, destPath)
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
	if err := os.MkdirAll(filepath.Dir(dest), 0755); err != nil {
		return fmt.Errorf("policy-admin: failed to create destination directory (%v)", err)
	}

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

/*
 * Extracts a .tar.gz policy archive into a temporary directory.
 * Validates paths to avoid traversal vulnerabilities (e.g., "../").
 */
func (self *PolicyAdminController) decompressPolicy() error {
	/* Create a temporary directory to store extracted policies */
	destDir, err := os.MkdirTemp(self.tempDir, "policy-*")
	if err != nil {
		return fmt.Errorf("policy-admin: can not create temp directory. (%v)", err)
	}
	self.uncompressedPolicyPath = destDir

	/* Open the compressed input file (.tar.gz) */
	file, err := os.Open(self.pulledPolicyPath)
	if err != nil {
		return err
	}
	defer file.Close()

	/* Initialize gzip reader */
	gzr, err := gzip.NewReader(file)
	if err != nil {
		return err
	}
	defer gzr.Close()

	/* Create tar reader to walk through archive contents */
	tr := tar.NewReader(gzr)

	for {
		/* Read next file header from the tar archive */
		header, err := tr.Next()
		if err == io.EOF {
			break /* No more files */
		}
		if err != nil {
			return err
		}

		/* Construct final extraction path */
		target := filepath.Join(destDir, header.Name)

		/* Prevent path traversal attacks using "../" */
		if !strings.Contains(target, "..") {
			switch header.Typeflag {

			/* Create directory targets */
			case tar.TypeDir:
				log.Debugf("policy-agent: creating directory: %s", target)
				if err := os.MkdirAll(target, 0775); err != nil {
					return err
				}

			/* Extract regular file targets */
			case tar.TypeReg:
				log.Debugf("policy-agent: extracting file: %s", target)
				if err := os.MkdirAll(filepath.Dir(target), 0775); err != nil {
					return err
				}

				outFile, err := os.OpenFile(target, os.O_CREATE|os.O_RDWR|os.O_TRUNC, 0775)
				if err != nil {
					return err
				}
				defer outFile.Close()

				/* Copy content from the archive into the destination file */
				if _, err := io.Copy(outFile, tr); err != nil {
					return err
				}
			}

		} else {
			/* Log suspicious path attempts */
			log.Warnf("policy-agent: invalid file path in tar archive: %s", target)
		}
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
