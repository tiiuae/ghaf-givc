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
	var firstChunk bool
	var metaDataStr string
	var tempFile *os.File
	var policyFilePath string
	for {
		req, err := stream.Recv()
		if err == io.EOF {
			fmt.Printf("New policy recieved\n")
			break
		}
		if err != nil {
			return err
		}

		if !firstChunk {
			firstChunk = true
			metaDataStr = req.GetMetadataJson()
			tempFile, err := os.CreateTemp(filepath.Join(PolicyBaseDir, ".temp"), "policy.bin-*")
			if err != nil {
				log.Errorf("policy-admin: failed to create temporary file: %v", err)
				return err
			}
			defer tempFile.Close()
			policyFilePath = tempFile.Name()
		} else {
			policyChunk := req.GetPolicyChunk()
			if _, err := tempFile.Write(policyChunk); err != nil {
				log.Errorf("policy-admin: failed to write to policy file: %v", err)
				return err
			}
		}
	}

	s.bgWG.Add(1)
	go func() {
		defer s.bgWG.Done()
		controller, err := NewPolicyAdminController(s.conf, metaDataStr, policyFilePath)
		if err != nil {
			log.Errorf("policy-admin: failed to create policy controller: %v", err)
			return
		}
		if err := controller.UpdatePolicies(); err != nil {
			log.Errorf("policy-admin: failed to handle policies: %v", err)
			controller.Cleanup()
			return
		}
	}()

	return stream.SendAndClose(&policyadmin.Status{Status: "Success"})
}
