// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package policyadmin

import (
	"fmt"
	"io"
	"os"
	"path/filepath"

	"givc/modules/api/policyadmin"
	pb "givc/modules/api/policyadmin"
	cfg "givc/modules/pkgs/config"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type PolicyAdminServer struct {
	pb.UnimplementedPolicyAdminServer
	policy     cfg.PolicyConfig
	controller *PolicyAdminController
}

func (s *PolicyAdminServer) Name() string {
	return "Policy Admin Server"
}

func (s *PolicyAdminServer) RegisterGrpcService(srv *grpc.Server) {
	pb.RegisterPolicyAdminServer(srv, s)
}

func NewPolicyAdminServer(policy cfg.PolicyConfig) (*PolicyAdminServer, error) {
	s := &PolicyAdminServer{
		policy:     policy,
		controller: nil,
	}
	var err error
	s.controller, err = NewPolicyAdminController(policy)
	if err != nil {
		return nil, err
	}
	return s, nil
}

func (s *PolicyAdminServer) StreamPolicy(stream pb.PolicyAdmin_StreamPolicyServer) error {
	/* Initialize state variables to track the streaming progress and file handling */
	var policyName string
	var tempFile *os.File
	var policyFilePath string

	log.Debugf("policy-admin:StreamPolicy()")
	firstChunk := true

	/* Receive stream chunks from the client */
	for {
		req, err := stream.Recv()

		/* Check for End of File (EOF), indicating the stream has finished successfully */
		if err == io.EOF {
			log.Debugf("policy-admin:StreamPolicy() policy downloaded successfully.\n")
			break
		}

		if err != nil {
			log.Errorf("policy-admin:StreamPolicy() policy download failed.")
			if tempFile != nil {
				tempFile.Close()
				os.Remove(policyFilePath)
			}
			return err
		}
		log.Debugf("policy-admin:StreamPolicy() chunk received.")

		/* The first chunk is metadata, and create the temporary file to store policy file */
		if firstChunk {
			firstChunk = false
			policyName = req.GetPolicyName()
			if policyName == "" {
				return fmt.Errorf("policy-admin: policy name is nil")
			}

			/* Ensure the temporary directory exists before creating the file */
			if err := os.MkdirAll(filepath.Join(s.policy.PolicyStorePath, ".temp"), 0755); err != nil {
				return fmt.Errorf("policy-admin: failed to create policy directory: %v", err)
			}
			log.Debugf("policy-admin:StreamPolicy() received policy: %s\n\n", policyName)

			/* Create a distinct temporary file to store the incoming binary data */
			tempFile, err = os.CreateTemp(filepath.Join(s.policy.PolicyStorePath, ".temp"), "policy.bin-*")
			if err != nil {
				log.Errorf("policy-admin: failed to create temporary file: %v", err)
				return err
			}

			/* Set file permissions to be readable */
			if err := tempFile.Chmod(0644); err != nil {
				log.Errorf("policy-admin: failed to set permissions on temporary file: %v", err)
				tempFile.Close()
				os.Remove(policyFilePath)
				return err
			}

			policyFilePath = tempFile.Name()
		}

		/* Handle subsequent chunks: Write the binary policy data to the temporary file */
		policyChunk := req.GetPolicyChunk()
		if policyChunk != nil {
			log.Debugf("policy-agent: writing chunk of %d bytes to temporary file....Chunk: %s\n\n", len(policyChunk), string(policyChunk))
			if _, err := tempFile.Write(policyChunk); err != nil {
				log.Errorf("policy-agent: failed to write to temporary file: %v", err)
				tempFile.Close()
				os.Remove(policyFilePath)
				return err
			}
		} else {
			log.Infof("policy-agent: policy chunk is nil")
		}
	}

	tempFile.Close()
	if err := s.controller.UpdatePolicy(policyName, policyFilePath); err != nil {
		if PathExists(policyFilePath) {
			os.Remove(policyFilePath)
		}
		log.Errorf("policy-admin: failed to update policy: %v", err)
		return err
	}

	/* Remove file policyfile if it exists */
	if PathExists(policyFilePath) {
		os.Remove(policyFilePath)
	}
	/* Send success status to the client and close the stream */
	return stream.SendAndClose(&policyadmin.Status{Status: "Success"})
}
