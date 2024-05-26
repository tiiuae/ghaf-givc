// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package main

import (
	"context"
	"crypto/tls"
	"flag"
	"givc/api/admin"
	givc_grpc "givc/internal/pkgs/grpc"
	"givc/internal/pkgs/types"
	givc_util "givc/internal/pkgs/utility"
	"os"
	"path/filepath"

	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc"
)

func main() {

	execName := filepath.Base(os.Args[0])
	log.Infof("Executing %s \n", execName)

	flag.Usage = func() {
		log.Infof("Usage of %s:\n", execName)
		log.Infof("%s [OPTIONS] \n", execName)
		flag.PrintDefaults()
	}

	name := flag.String("name", "<application>", "Name of application to start")
	host := flag.String("host", "admin.local", "Name of admin server to receive commands")
	address := flag.String("ip", "127.0.0.1", "Host ip")
	port := flag.String("port", "9000", "Host port")
	protocol := flag.String("protocol", "tcp", "Transport protocol")
	cacert := flag.String("ca", "/etc/ssl/certs/ca-certificates.crt", "CA certificate")
	cert := flag.String("cert", "/etc/ssl/certs/app.crt", "Client certificate")
	key := flag.String("key", "/etc/ssl/certs/app.key", "Client key")
	notls := flag.Bool("notls", false, "Disable TLS")

	flag.Parse()

	var tlsConfig *tls.Config
	if !*notls {
		tlsConfig = givc_util.TlsClientConfig(*cacert, *cert, *key, *host)
	}

	cfgAdminServer := &types.EndpointConfig{
		Transport: types.TransportConfig{
			Name:     *host,
			Address:  *address,
			Port:     *port,
			Protocol: *protocol,
		},
		TlsConfig: tlsConfig,
	}

	// Setup and dial GRPC client
	var conn *grpc.ClientConn
	conn, err := givc_grpc.NewClient(cfgAdminServer, false)
	if err != nil {
		log.Fatalf("Cannot create grpc client: %v", err)
	}
	defer conn.Close()

	// Create client
	client := admin.NewAdminServiceClient(conn)
	if client == nil {
		log.Fatalf("Failed to create 'NewAdminServiceClient'")
	}

	ctx := context.Background()
	switch *name {
	case "poweroff":
		_, err := client.Poweroff(ctx, &admin.Empty{})
		if err != nil {
			log.Errorf("Error executing poweroff: %s", err)
		}
	case "reboot":
		_, err := client.Reboot(ctx, &admin.Empty{})
		if err != nil {
			log.Errorf("Error executing reboot: %s", err)
		}
	default:
		req := &admin.ApplicationRequest{
			AppName: *name,
		}
		resp, err := client.StartApplication(ctx, req)
		if err != nil {
			log.Errorf("Error executing application: %s", err)
		}
		log.Infoln(resp)
	}

}
