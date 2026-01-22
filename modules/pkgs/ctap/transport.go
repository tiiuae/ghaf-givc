// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package ctap

import (
	"context"
	"errors"
	"io"
	"os/exec"
	"sync"

	log "github.com/sirupsen/logrus"
	"givc/modules/api/ctap"
	"google.golang.org/grpc"
)

type process struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	stdout io.ReadCloser
	stderr io.ReadCloser
}

type CtapServer struct {
	ctap.UnimplementedCtapServer
	processes sync.Map
}

func (s *CtapServer) Name() string {
	return "Ctap Server"
}

func (s *CtapServer) RegisterGrpcService(srv *grpc.Server) {
	ctap.RegisterCtapServer(srv, s)
}

func NewCtapServer() (*CtapServer, error) {
	ctapServer := CtapServer{}

	return &ctapServer, nil
}

func (s *CtapServer) Ctap(ctx context.Context, req *ctap.CtapRequest) (*ctap.CtapResponse, error) {
	var prog string

	log.Infof("[Ctap] got request %v+%v", req.Req, req.Args)

	switch req.Req {
	case "ctap.ClientPin":
		prog = "qctap-client-pin"
	case "ctap.GetInfo":
		prog = "qctap-get-info"
	case "u2f.Authenticate":
		prog = "qctap-get-assertion"
	case "u2f.Register":
		prog = "qctap-make-credential"
	default:
		return nil, errors.New("Invalid request")
	}

	cmd := exec.Command(prog, req.Args...)

	stdin, err := cmd.StdinPipe()
	if err != nil {
		log.Warnf("[Ctap] stdin pipe failed: %v", err)
		return nil, err
	}

	go func() {
		defer stdin.Close()
		if _, err := stdin.Write(req.Payload); err != nil {
			log.Warnf("[Ctap] failed to write payload to child: %v", err)
		}
	}()

	output, err := cmd.Output()
	if err != nil {
		// Return output anyway, even if request failed
		log.Warnf("[Ctap] helper failed: %v", err)
	}

	return &ctap.CtapResponse{Output: output}, nil
}
