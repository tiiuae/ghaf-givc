// SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
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

	if err := s.Controller.SetLocale(ctx, req.Assignments); err != nil {
		log.Errorf("[LocaleSet] Error setting locale: %v", err)
		return nil, fmt.Errorf("cannot set locale: %w", err)
	}

	return &givc_locale.Empty{}, nil
}

// TimezoneSet handles incoming requests to set the timezone.
func (s *LocaleServer) TimezoneSet(ctx context.Context, req *givc_locale.TimezoneMessage) (*givc_locale.Empty, error) {
	log.Infof("Incoming notification of set timezone\n")

	// Same as LocaleSet above; the label also said "[SetLocale]".
	if err := s.Controller.SetTimezone(ctx, req.Timezone); err != nil {
		log.Errorf("[TimezoneSet] Error setting timezone: %v", err)
		return nil, fmt.Errorf("cannot set timezone: %w", err)
	}

	return &givc_locale.Empty{}, nil
}
