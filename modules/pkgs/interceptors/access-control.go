// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

package interceptors

import (
	"context"
	"fmt"
	"net"
	"os"
	"reflect"
	"strings"

	"github.com/cedar-policy/cedar-go"
	cedartypes "github.com/cedar-policy/cedar-go/types"
	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"
)

func toCedarValue(v reflect.Value) (cedartypes.Value, bool) {
	if v.Kind() == reflect.Ptr {
		if v.IsNil() {
			return nil, false
		}
		v = v.Elem()
	}

	switch v.Kind() {
	case reflect.String:
		return cedartypes.String(v.String()), true
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return cedartypes.Long(v.Int()), true
	case reflect.Bool:
		return cedartypes.Boolean(v.Bool()), true
	case reflect.Slice:
		elements := make([]cedartypes.Value, 0, v.Len())
		for j := 0; j < v.Len(); j++ {
			if val, ok := toCedarValue(v.Index(j)); ok {
				elements = append(elements, val)
			}
		}
		return cedartypes.NewSet(elements...), true
	case reflect.Struct:
		// Map nested struct to Cedar Record
		return cedartypes.NewRecord(MapRequestToContext(v.Interface())), true
	default:
		return nil, false
	}
}

func MapRequestToContext(req interface{}) cedartypes.RecordMap {
	v := reflect.ValueOf(req)
	if v.Kind() == reflect.Ptr {
		if v.IsNil() {
			return make(cedartypes.RecordMap)
		}
		v = v.Elem()
	}
	if v.Kind() != reflect.Struct {
		return make(cedartypes.RecordMap)
	}

	ctxMap := make(cedartypes.RecordMap)
	t := v.Type()
	for i := 0; i < v.NumField(); i++ {
		field := v.Field(i)
		fieldType := t.Field(i)
		fieldName := fieldType.Name

		if !fieldType.IsExported() {
			continue
		}
		if _, ok := fieldType.Tag.Lookup("protobuf"); !ok {
			continue
		}

		if val, ok := toCedarValue(field); ok {
			ctxMap[cedartypes.String(fieldName)] = val
		}
	}
	return ctxMap
}

func getSource(ctx context.Context) string {
	host := "unknown"
	if p, ok := peer.FromContext(ctx); ok {
		if tlsInfo, ok := p.AuthInfo.(credentials.TLSInfo); ok {
			if len(tlsInfo.State.PeerCertificates) > 0 {
				cert := tlsInfo.State.PeerCertificates[0]
				if len(cert.DNSNames) > 0 {
					name := cert.DNSNames[0]
					if strings.HasPrefix(name, "DNS.1:") {
						name = strings.TrimPrefix(name, "DNS.1:")
						if idx := strings.Index(name, ","); idx != -1 {
							name = name[:idx]
						}
					}
					log.Infof("Authorizing with principal from certificate SAN DNSName: %s", name)
					return name
				}
			}
		}
		// ipaddress/vsock cid
		host, _, splitErr := net.SplitHostPort(p.Addr.String())
		if splitErr != nil {
			host = p.Addr.String()
		}
		if ip := net.ParseIP(host); ip != nil {
			log.Infof("Authorizing with principal from peer IP: %s", host)
			return host
		}
	}

	return host
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
		source := getSource(ctx)
		firstSplit := strings.SplitN(strings.TrimPrefix(fullMethod, "/"), "/", 2)
		secondSplit := strings.SplitN(firstSplit[0], ".", 2)
		method := firstSplit[1]
		moduleName := secondSplit[0]
		grpcService := secondSplit[1]

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
			}).Warnf("Cedar authorization denied")
			return status.Errorf(codes.PermissionDenied, "permission denied by access control policy")
		}
		log.WithFields(log.Fields{
			"principal": source,
			"module":    moduleName,
			"action":    method,
			"context":   cedarCtx,
		}).Warnf("Cedar authorization granted") //TODO:Infof
		return nil
	}

	// Unary Interceptor
	unary := func(ctx context.Context, req interface{}, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (interface{}, error) {
		if err := authorize(ctx, info.FullMethod, MapRequestToContext(req)); err != nil {
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
