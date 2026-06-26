// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package interceptors

import (
	"context"
	"crypto/tls"

	givc_types "givc/modules/pkgs/types"
	givc_util "givc/modules/pkgs/utility"

	grpc_logrus "github.com/grpc-ecosystem/go-grpc-middleware/logging/logrus"
	grpc_ctxtags "github.com/grpc-ecosystem/go-grpc-middleware/tags"
	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

func unaryLogRequestInterceptor(ctx context.Context, req interface{}, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (interface{}, error) {
	log.WithFields(grpc_ctxtags.Extract(ctx).Values()).Info("GRPC Request: ", info.FullMethod)
	return handler(ctx, req)
}

func GetServerInterceptors(acConfig *givc_types.AccessControl, tlsConfig *tls.Config) ([]grpc.UnaryServerInterceptor, []grpc.StreamServerInterceptor, error) {
	unaryInterceptors := []grpc.UnaryServerInterceptor{
		grpc_ctxtags.UnaryServerInterceptor(grpc_ctxtags.WithFieldExtractor(grpc_ctxtags.TagBasedRequestFieldExtractor("log"))),
		unaryLogRequestInterceptor,
		grpc_logrus.UnaryServerInterceptor(log.NewEntry(log.StandardLogger())),
	}

	streamInterceptors := []grpc.StreamServerInterceptor{
		grpc_ctxtags.StreamServerInterceptor(grpc_ctxtags.WithFieldExtractor(grpc_ctxtags.TagBasedRequestFieldExtractor("log"))),
		grpc_logrus.StreamServerInterceptor(log.NewEntry(log.StandardLogger())),
	}

	if tlsConfig != nil {
		unaryInterceptors = append(unaryInterceptors, givc_util.CertIPVerifyUnaryInterceptor)
		streamInterceptors = append(streamInterceptors, givc_util.CertIPVerifyStreamInterceptor)
	}

	if acConfig != nil && acConfig.AccessControlEnabled && acConfig.RulesFile != "" {
		uI, sI, err := NewAccessController(acConfig.RulesFile)
		if err != nil {
			return nil, nil, err
		}
		unaryInterceptors = append(unaryInterceptors, uI)
		streamInterceptors = append(streamInterceptors, sI)
		log.Info("Cedar access control interceptors loaded")
	}

	return unaryInterceptors, streamInterceptors, nil
}
