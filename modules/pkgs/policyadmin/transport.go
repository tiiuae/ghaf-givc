// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package policyadmin

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sync"

	"givc/modules/api/policyadmin"
	pb "givc/modules/api/policyadmin"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type PolicyAdminServer struct {
	pb.UnimplementedPolicyAdminServer

	conf string
	bgWG sync.WaitGroup
}

func (s *PolicyAdminServer) Name() string {
	return "Policy Admin Server"
}

func (s *PolicyAdminServer) RegisterGrpcService(srv *grpc.Server) {
	pb.RegisterPolicyAdminServer(srv, s)
}

func NewPolicyAdminServer(conf string) (*PolicyAdminServer, error) {
	s := &PolicyAdminServer{
		conf: conf,
	}
	return s, nil
}

func (s *PolicyAdminServer) WaitForBackgroundJobs() {
	s.bgWG.Wait()
}

func (s *PolicyAdminServer) StreamPolicy(stream pb.PolicyAdmin_StreamPolicyServer) error {
	/* Initialize state variables to track the streaming progress and file handling */
	var firstChunk bool
	var metaDataStr string
	var tempFile *os.File
	var policyFilePath string

	// log.SetLevel(log.DebugLevel)
	log.Infof("policy-admin:StreamPolicy()")

	/* Receive stream chunks from the client */
	for {
		req, err := stream.Recv()

		/* Check for End of File (EOF), indicating the stream has finished successfully */
		if err == io.EOF {
			log.Infof("policy-admin:StreamPolicy() policy downloaded successfully.\n")
			break
		}

		if err != nil {
			log.Infof("policy-admin:StreamPolicy() policy download failed.")
			return err
		}
		log.Infof("policy-admin:StreamPolicy() chunk received.")

		/* The first chunk is metadata, and create the temporary file to store policy file */
		if !firstChunk {
			firstChunk = true
			metaDataStr = req.GetMetadataJson()

			/* Ensure the temporary directory exists before creating the file */
			if err := os.MkdirAll(filepath.Join(PolicyBaseDir, ".temp"), 0755); err != nil {
				return fmt.Errorf("policy-admin: failed to create policy directory: %v", err)
			}
			log.Infof("policy-admin:StreamPolicy() metadata received: %s\n\n", metaDataStr)

			/* Log existence of policy chunk in first packet, though usually reserved for metadata */
			policyChunk := req.GetPolicyChunk()
			if policyChunk != nil {
				log.Infof("Policy chunk is not nil in first chunk")
			}

			/* Create a distinct temporary file to store the incoming binary data */
			tempFile, err = os.CreateTemp(filepath.Join(PolicyBaseDir, ".temp"), "policy.bin-*")
			if err != nil {
				log.Errorf("policy-admin: failed to create temporary file: %v", err)
				return err
			}
			/* Ensure the file handle is closed when the function exits */
			defer tempFile.Close()

			/* Set file permissions to be readable */
			if err := tempFile.Chmod(0644); err != nil {
				log.Errorf("policy-admin: failed to set permissions on temporary file: %v", err)
				return err
			}

			policyFilePath = tempFile.Name()
		} else {
			/* Handle subsequent chunks: Write the binary policy data to the temporary file */
			policyChunk := req.GetPolicyChunk()
			if policyChunk != nil {
				log.Infof("policy-agent: writing chunk of %d bytes to temporary file....Chunk: %s\n\n", len(policyChunk), string(policyChunk))
				if _, err := tempFile.Write(policyChunk); err != nil {
					log.Errorf("policy-agent: failed to write to temporary file: %v", err)
					return err
				}
			} else {
				log.Infof("policy-agent: policy chunk is nil")
			}

		}
	}

	/* Use a WaitGroup to track the background processing logic */
	s.bgWG.Add(1)

	/* execute policy updates asynchronously to avoid blocking the gRPC response */
	go func() {
		defer s.bgWG.Done()
		/* Initialize the controller with the downloaded file and metadata */
		controller, err := NewPolicyAdminController(s.conf, metaDataStr, policyFilePath)
		if err != nil {
			log.Errorf("policy-admin: failed to create policy controller: %v", err)
			return
		}
		/* Attempt to apply the new policies */
		if err := controller.UpdatePolicies(); err != nil {
			log.Errorf("policy-admin: failed to handle policies: %v", err)
			controller.Cleanup()
			return
		}
	}()

	/* Send success status to the client and close the stream */
	return stream.SendAndClose(&policyadmin.Status{Status: "Success"})
}
