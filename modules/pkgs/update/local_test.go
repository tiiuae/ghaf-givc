// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package update

import (
	"context"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	pb "givc/modules/api/update"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/test/bufconn"
)

func localClient(t *testing.T, role string) (pb.UpdateClient, string) {
	t.Helper()
	dir := t.TempDir()
	script := `#!/bin/sh
set -eu
case "$3" in
receive) IFS= read -r header; cat > "$LOCAL_TEST_DIR/received" ;;
discard-received) touch "$LOCAL_TEST_DIR/cleaned" ;;
install) sleep 0.1; printf 'output after disconnect'; touch "$LOCAL_TEST_DIR/installed" ;;
*) exit 1 ;;
esac
`
	if err := os.WriteFile(filepath.Join(dir, "ota-update"), []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir+":"+os.Getenv("PATH"))
	t.Setenv("LOCAL_TEST_DIR", dir)
	t.Setenv("GHAF_LOCAL_UPDATE_ROLE", role)
	listener := bufconn.Listen(1024 * 1024)
	server := grpc.NewServer()
	pb.RegisterUpdateServer(server, &UpdateServer{})
	go server.Serve(listener)
	t.Cleanup(server.Stop)
	conn, err := grpc.NewClient("passthrough:///local", grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithContextDialer(func(context.Context, string) (net.Conn, error) { return listener.Dial() }))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { conn.Close() })
	return pb.NewUpdateClient(conn), dir
}

func TestLocalReceiveRequiresExplicitCompletionAndBoundedChunks(t *testing.T) {
	for _, test := range []struct {
		name   string
		data   []byte
		finish bool
		valid  bool
	}{
		{"complete", []byte("artifact bytes"), true, true},
		{"disconnect", []byte("artifact bytes"), false, false},
		{"oversized", make([]byte, localChunkSize+1), true, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			client, dir := localClient(t, "receiver")
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			stream, err := client.ReceiveLocalImport(ctx)
			if err != nil {
				t.Fatal(err)
			}
			first := &pb.LocalReceiveRequest{Id: strings.Repeat("ab", 16), Metadata: &pb.LocalMetadata{Manifest: "{}", Signature: strings.Repeat("00", 64)}}
			if err := stream.Send(first); err != nil {
				t.Fatal(err)
			}
			if err := stream.Send(&pb.LocalReceiveRequest{Data: test.data}); err != nil {
				t.Fatal(err)
			}
			if test.finish {
				_ = stream.Send(&pb.LocalReceiveRequest{Finish: true})
			}
			_ = stream.CloseSend()
			completed := false
			for {
				event, err := stream.Recv()
				if err != nil {
					if test.valid && err != io.EOF {
						t.Fatal(err)
					}
					if !test.valid && err == io.EOF {
						t.Fatal("invalid upload succeeded")
					}
					break
				}
				completed = completed || event.Phase == "received"
			}
			if completed != test.valid {
				t.Fatalf("completed=%v, want %v", completed, test.valid)
			}
			if test.valid {
				got, err := os.ReadFile(filepath.Join(dir, "received"))
				if err != nil || string(got) != string(test.data) {
					t.Fatalf("data=%q error=%v", got, err)
				}
			} else if _, err := os.Stat(filepath.Join(dir, "cleaned")); err != nil {
				t.Fatal("failed import was not cleaned:", err)
			}
		})
	}
}

func TestLocalRolesAndPathIDsAreEnforced(t *testing.T) {
	client, _ := localClient(t, "receiver")
	if _, err := client.PrepareLocalImport(context.Background(), &pb.LocalMetadata{}); err == nil {
		t.Fatal("receiver admitted host request")
	}
	if _, err := client.DiscardLocalImport(context.Background(), &pb.LocalCandidateRequest{Id: "../outside"}); err == nil {
		t.Fatal("unsafe ID accepted")
	}
}

func TestLocalInstallContinuesAfterSubscriberDisconnect(t *testing.T) {
	client, dir := localClient(t, "host")
	ctx, cancel := context.WithCancel(context.Background())
	stream, err := client.InstallCandidate(ctx, &pb.LocalCandidateRequest{Id: strings.Repeat("ab", 16)})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := stream.Recv(); err != nil {
		t.Fatal(err)
	}
	cancel()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(filepath.Join(dir, "installed")); err == nil {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("installation stopped when progress subscriber disconnected")
}
