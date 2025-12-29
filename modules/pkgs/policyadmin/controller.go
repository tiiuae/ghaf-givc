// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package policyadmin

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"

	cfg "givc/modules/pkgs/config"

	log "github.com/sirupsen/logrus"
)

type PolicyDetail struct {
	Destination string
	Sha         string
}

/*
 * PolicyAdminController coordinates policy updates for vm-policies.
 * It handles the incoming policy payload, extracts archive,
 * manages revision state, deployment targets, and concurrency control while
 * applying policy updates.
 */
type PolicyAdminController struct {
	tempDir          string
	policiesInfoFile string
	storePath        string
	policyMap        map[string]*PolicyDetail
}

/* Constructor */
func NewPolicyAdminController(policy cfg.PolicyConfig) (*PolicyAdminController, error) {
	log.Debugf("policy-admin:NewPolicyAdminController()")
	self := &PolicyAdminController{}
	self.storePath = policy.PolicyStorePath
	self.policiesInfoFile = filepath.Join(policy.PolicyStorePath, "policies.json")
	self.tempDir = filepath.Join(policy.PolicyStorePath, ".temp")
	self.loadPolicyMap(policy)
	return self, nil
}

/* Updates the policies to the targets as per defined in config */
func (self *PolicyAdminController) UpdatePolicy(policyName string, pulledPolicyPath string) error {
	policyDir := filepath.Join(self.storePath, policyName)
	policyFile := filepath.Join(policyDir, "policy.bin")
	policy, exists := self.policyMap[policyName]

	if !exists {
		return fmt.Errorf("Unknown policy %s.", policyName)
	}

	fileHash := policy.Sha

	if !PathExists(policyFile) {
		/* Create or ensure the local policy directory exists */
		if err := os.MkdirAll(policyDir, 0755); err != nil {
			return fmt.Errorf("policy-admin: failed to create policy directory: %v", err)
		}
	}

	pulledPolicyHash, err := getFileHash(pulledPolicyPath)
	if err != nil {
		return err
	}

	if fileHash == pulledPolicyHash {
		return nil
	}

	if policy.Destination == "" {
		/* Move downloaded file to local store (atomic replacement) */
		if err := os.Rename(pulledPolicyPath, policyFile); err != nil {
			return fmt.Errorf("policy-admin: failed to write policy locally: %v", err)
		}

		log.Infof("policy-admin: no destination configured for '%s', installation only local", policyName)
		self.policyMap[policyName].Sha = pulledPolicyHash
		self.savePolicyMap()
		return nil
	}

	/* Copy the file to the defined destination */
	if err := self.copyPolicy(pulledPolicyPath, policy.Destination); err != nil {
		return fmt.Errorf("policy-admin: failed to deploy policy file: %v", err)
	}

	/* Move downloaded file to local store (atomic replacement) */
	if err := os.Rename(pulledPolicyPath, policyFile); err != nil {
		return fmt.Errorf("policy-admin: failed to write policy locally: %v", err)
	}

	self.policyMap[policyName].Sha = pulledPolicyHash
	self.savePolicyMap()
	log.Infof("policy-admin: installed %s → %s", policyFile, policy.Destination)
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

func (self *PolicyAdminController) savePolicyMap() error {
	jsonData, err := json.MarshalIndent(self.policyMap, "", "  ")
	if err != nil {
		return err
	}

	return os.WriteFile(self.policiesInfoFile, jsonData, 0644)
}

func (self *PolicyAdminController) loadPolicyMap(policy cfg.PolicyConfig) error {
	if !PathExists(self.policiesInfoFile) {
		self.policyMap = make(map[string]*PolicyDetail)
		for name, dest := range policy.PoliciesJson {
			self.policyMap[name] = &PolicyDetail{
				Destination: dest,
				Sha:         "",
			}
		}
		return nil
	}
	jsonData, err := os.ReadFile(self.policiesInfoFile)
	if err != nil {
		return err
	}

	self.policyMap = make(map[string]*PolicyDetail)
	err = json.Unmarshal(jsonData, &self.policyMap)
	if err != nil {
		return err
	}

	return nil
}

func getFileHash(filePath string) (string, error) {
	file, err := os.Open(filePath)
	if err != nil {
		return "", err
	}
	defer file.Close()

	hash := sha256.New()

	if _, err := io.Copy(hash, file); err != nil {
		return "", err
	}

	return hex.EncodeToString(hash.Sum(nil)), nil
}

/* PathExists returns true if the given filesystem path exists. */
func PathExists(path string) bool {
	_, err := os.Stat(path)
	if err == nil {
		return true
	}
	return !os.IsNotExist(err)
}
