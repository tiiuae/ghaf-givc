// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Package localelistener provides functionality to listen for locale and timezone changes.
package localelistener

import (
	"context"
	"fmt"

	givc_locale "givc/modules/api/locale"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type LocaleServer struct {
	Controller *LocaleController
	givc_locale.UnimplementedLocaleClientServer
}

func (s *LocaleServer) Name() string {
	return "Locale listener"
}

func (s *LocaleServer) RegisterGrpcService(srv *grpc.Server) {
	givc_locale.RegisterLocaleClientServer(srv, s)
}

// NewLocaleServer creates a new instance of LocaleServer.
func NewLocaleServer() (*LocaleServer, error) {

	localeController, err := NewController()
	if err != nil {
		log.Errorf("Error creating locale controller: %v", err)
		return nil, err
	}

	localeServer := LocaleServer{
		Controller: localeController,
	}

	return &localeServer, nil
}

// LocaleSet handles incoming requests to set the locale.
func (s *LocaleServer) LocaleSet(ctx context.Context, req *givc_locale.LocaleMessage) (*givc_locale.Empty, error) {
	log.Infof("Incoming notification of changes locale\n")

	err := s.Controller.SetLocale(context.Background(), req.Locale)
	if err != nil {
		log.Infof("[SetLocale] Error setting locale: %v\n", err)
		return nil, fmt.Errorf("cannot set locale")
	}

	return &givc_locale.Empty{}, nil
}

// TimezoneSet handles incoming requests to set the timezone.
func (s *LocaleServer) TimezoneSet(ctx context.Context, req *givc_locale.TimezoneMessage) (*givc_locale.Empty, error) {
	log.Infof("Incoming notification of set timezone\n")

	err := s.Controller.SetTimezone(context.Background(), req.Timezone)
	if err != nil {
		log.Infof("[SetLocale] Error setting timezone: %v\n", err)
		return nil, fmt.Errorf("cannot set timezone")
	}

	return &givc_locale.Empty{}, nil
}
