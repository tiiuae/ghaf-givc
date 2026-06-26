// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package interceptors

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"strings"

	"github.com/cedar-policy/cedar-go"
	cedartypes "github.com/cedar-policy/cedar-go/types"
	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"
)

func MapRequestToContext(req proto.Message) (cedartypes.RecordMap, error) {
	b, err := protojson.MarshalOptions{UseProtoNames: true}.Marshal(req)
	if err != nil {
		return cedartypes.RecordMap{}, err
	}
	var record cedartypes.Record
	if err := json.Unmarshal(b, &record); err != nil {
		return cedartypes.RecordMap{}, err
	}
	rm := record.Map()
	if rm == nil {
		return cedartypes.RecordMap{}, nil
	}
	return rm, nil
}

func getSource(ctx context.Context) (string, error) {
	p, ok := peer.FromContext(ctx)
	if !ok {
		return "", fmt.Errorf("no peer info available in context")
	}

	if tlsInfo, ok := p.AuthInfo.(credentials.TLSInfo); ok && len(tlsInfo.State.PeerCertificates) > 0 {
		cert := tlsInfo.State.PeerCertificates[0]
		if len(cert.DNSNames) > 0 {
			name := cert.DNSNames[0]
			name = strings.TrimSpace(name)
			if name == "" {
				return "", fmt.Errorf("invalid DNS SAN: DNS name not found")
			}

			name, _, _ = strings.Cut(name, ",") //First name only
			if name == "" {
				return "", fmt.Errorf("invalid DNS SAN")
			}

			log.Infof("Authorizing with principal from certificate SAN DNSName: %s", name)
			return name, nil
		}
	}

	// ipaddress/vsock cid
	host, _, err := net.SplitHostPort(p.Addr.String())
	if err != nil {
		host = p.Addr.String()
	}
	if ip := net.ParseIP(host); ip != nil {
		log.Infof("Authorizing with principal from peer IP: %s", host)
		return host, nil
	}

	// Replaced "unknown" with empty string and an explicit error
	return "", fmt.Errorf("unable to determine source principal from peer connection")
}

func NewAccessController(policyPath string) (grpc.UnaryServerInterceptor, grpc.StreamServerInterceptor, error) {
	policyBytes, err := os.ReadFile(policyPath)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to read cedar policy file: %w", err)
	}

	policies, err := cedar.NewPolicySetFromBytes("policy0", policyBytes)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to parse cedar policy: %w", err)
	}

	authorize := func(ctx context.Context, fullMethod string, ctxMap cedartypes.RecordMap) error {
		if fullMethod == "" {
			return status.Error(codes.InvalidArgument, "fullMethod cannot be empty")
		}

		var source string
		if source, err = getSource(ctx); err != nil {
			return status.Errorf(codes.PermissionDenied, "Cedar: unknow source! error: %v", err)
		}

		trimmedMethod := strings.TrimPrefix(fullMethod, "/")
		if trimmedMethod == "" {
			return status.Errorf(codes.InvalidArgument, "invalid gRPC method format: %s", fullMethod)
		}

		servicePart, method, found := strings.Cut(trimmedMethod, "/")
		if !found || servicePart == "" || method == "" {
			return status.Errorf(codes.InvalidArgument, "failed to parse method from fullMethod: %s", fullMethod)
		}

		moduleName, grpcService, found := strings.Cut(servicePart, ".")
		if !found || moduleName == "" || grpcService == "" {
			return status.Errorf(codes.InvalidArgument, "failed to parse module and service from fullMethod: %s", fullMethod)
		}

		ctxMap["service"] = cedartypes.String(grpcService)
		cedarCtx := cedartypes.NewRecord(ctxMap)

		request := cedar.Request{
			Principal: cedartypes.NewEntityUID("Source", cedartypes.String(source)),
			Resource:  cedartypes.NewEntityUID("Module", cedartypes.String(moduleName)),
			Action:    cedartypes.NewEntityUID("Command", cedartypes.String(method)),
			Context:   cedarCtx,
		}

		ok, diag := cedar.Authorize(policies, cedar.EntityMap{}, request)
		if ok != cedartypes.Allow {
			log.WithFields(log.Fields{
				"principal": source,
				"module":    moduleName,
				"action":    method,
				"context":   cedarCtx,
				"errors":    diag.Errors,
			}).Errorf("Cedar authorization denied")
			return status.Errorf(codes.PermissionDenied, "permission denied by access control policy")
		}
		log.WithFields(log.Fields{
			"principal": source,
			"module":    moduleName,
			"action":    method,
			"context":   cedarCtx,
		}).Debugf("Cedar authorization granted")
		return nil
	}

	// Unary Interceptor
	unary := func(ctx context.Context, req interface{}, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (interface{}, error) {
		protoReq, ok := req.(proto.Message)
		if !ok {
			return nil, status.Error(codes.Internal, "request is not a proto.Message")
		}
		record, err := MapRequestToContext(protoReq)
		if err != nil {
			return nil, status.Error(codes.Internal, "failed to map request to cedar context")
		}
		if err := authorize(ctx, info.FullMethod, record); err != nil {
			return nil, err
		}
		return handler(ctx, req)
	}

	// Stream Interceptor
	stream := func(srv interface{}, ss grpc.ServerStream, info *grpc.StreamServerInfo, handler grpc.StreamHandler) error {
		// Note: req is nil here because we don't have the first message yet for all stream types. So we use an empty context.
		if err := authorize(ss.Context(), info.FullMethod, make(cedartypes.RecordMap)); err != nil {
			return err
		}
		return handler(srv, ss)
	}

	return unary, stream, nil
}
