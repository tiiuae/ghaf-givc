// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package localelistener

import (
	"context"
	"fmt"

	locale_api "givc/api/locale"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

type LocaleServer struct {
	Controller *LocaleController
	locale_api.UnimplementedLocaleClientServer
}

func (s *LocaleServer) Name() string {
	return "Locale listener"
}

func (s *LocaleServer) RegisterGrpcService(srv *grpc.Server) {
	locale_api.RegisterLocaleClientServer(srv, s)
}

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

func (s *LocaleServer) LocaleSet(ctx context.Context, req *locale_api.LocaleMessage) (*locale_api.Empty, error) {
	log.Infof("Incoming notification of changes locale\n")

	err := s.Controller.SetLocale(context.Background(), req.Locale)
	if err != nil {
		log.Infof("[SetLocale] Error setting locale: %v\n", err)
		return nil, fmt.Errorf("Cannot set locale")
	}

	return &locale_api.Empty{}, nil
}

func (s *LocaleServer) TimezoneSet(ctx context.Context, req *locale_api.TimezoneMessage) (*locale_api.Empty, error) {
	log.Infof("Incoming notification of set timezone\n")

	err := s.Controller.SetTimezone(context.Background(), req.Timezone)
	if err != nil {
		log.Infof("[SetLocale] Error setting timezone: %v\n", err)
		return nil, fmt.Errorf("Cannot set timezone")
	}

	return &locale_api.Empty{}, nil
}
