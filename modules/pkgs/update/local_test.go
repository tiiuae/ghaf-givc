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
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
)

func localClient(t *testing.T, role string, idle ...time.Duration) (pb.UpdateClient, string) {
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
	if len(idle) > 0 {
		pb.RegisterUpdateServer(server, &idleUpdateServer{idle: idle[0]})
	} else {
		pb.RegisterUpdateServer(server, &UpdateServer{})
	}
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

// Short deadlines exercise the same receive handler without slowing production timeouts.
type idleUpdateServer struct {
	UpdateServer
	idle time.Duration
}

func (s *idleUpdateServer) ReceiveLocalImport(stream pb.Update_ReceiveLocalImportServer) error {
	return s.receiveLocalImport(stream, s.idle)
}

func TestLocalReceiveIdleTimeoutCleansStaging(t *testing.T) {
	client, dir := localClient(t, "receiver", 100*time.Millisecond)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	stream, err := client.ReceiveLocalImport(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := stream.Send(&pb.LocalReceiveRequest{Id: strings.Repeat("ab", 16), Metadata: &pb.LocalMetadata{Manifest: "{}", Signature: strings.Repeat("00", 64)}}); err != nil {
		t.Fatal(err)
	}
	if _, err := stream.Recv(); err != nil {
		t.Fatal(err)
	}
	_, err = stream.Recv()
	if status.Code(err) != codes.DeadlineExceeded {
		t.Fatalf("expected idle deadline, got %v", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "cleaned")); err != nil {
		t.Fatal("staging not cleaned:", err)
	}
}

func TestLocalReceivePreservesEarlyChildDiagnostic(t *testing.T) {
	client, dir := localClient(t, "receiver")
	script := "#!/bin/sh\nif [ \"$3\" = receive ]; then echo 'insufficient staging space' >&2; exit 1; fi\n"
	if err := os.WriteFile(filepath.Join(dir, "ota-update"), []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	stream, err := client.ReceiveLocalImport(ctx)
	if err != nil {
		t.Fatal(err)
	}
	_ = stream.Send(&pb.LocalReceiveRequest{Id: strings.Repeat("ab", 16), Metadata: &pb.LocalMetadata{Manifest: "{}", Signature: strings.Repeat("00", 64)}})
	_ = stream.Send(&pb.LocalReceiveRequest{Data: make([]byte, localChunkSize)})
	_ = stream.CloseSend()
	for {
		_, err := stream.Recv()
		if err != nil {
			if !strings.Contains(err.Error(), "insufficient staging space") {
				t.Fatalf("lost child diagnostic: %v", err)
			}
			break
		}
	}
}

func TestLocalInstallStreamsBothOutputsBeforeCompletion(t *testing.T) {
	client, dir := localClient(t, "host")
	script := "#!/bin/sh\necho 'stdout progress'; echo 'stderr progress' >&2; sleep 1; touch \"$LOCAL_TEST_DIR/installed\"\n"
	if err := os.WriteFile(filepath.Join(dir, "ota-update"), []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	stream, err := client.InstallCandidate(ctx, &pb.LocalCandidateRequest{Id: strings.Repeat("ab", 16)})
	if err != nil {
		t.Fatal(err)
	}
	var output string
	for !strings.Contains(output, "stdout progress") || !strings.Contains(output, "stderr progress") {
		event, err := stream.Recv()
		if err != nil {
			t.Fatal(err)
		}
		output += event.GetOutput()
	}
	if _, err := os.Stat(filepath.Join(dir, "installed")); !os.IsNotExist(err) {
		t.Fatal("progress arrived only after installer finished")
	}
	for {
		if _, err := stream.Recv(); err != nil {
			if err != io.EOF {
				t.Fatal(err)
			}
			break
		}
	}
}

func TestImageInstallRPCUsesSupportedArguments(t *testing.T) {
	client, dir := localClient(t, "host")
	script := "#!/bin/sh\nprintf '%s\\n' \"$*\" >\"$LOCAL_TEST_DIR/arguments\"\ntest \"$#\" = 4 && test \"$1 $2 $3 $4\" = 'image install --manifest /tmp/update.manifest'\n"
	if err := os.WriteFile(filepath.Join(dir, "ota-update"), []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	stream, err := client.ImageInstall(ctx, &pb.ImageInstallRequest{Manifest: "/tmp/update.manifest"})
	if err != nil {
		t.Fatal(err)
	}
	for {
		if _, err := stream.Recv(); err != nil {
			if err != io.EOF {
				t.Fatal(err)
			}
			break
		}
	}
	args, err := os.ReadFile(filepath.Join(dir, "arguments"))
	if err != nil || string(args) != "image install --manifest /tmp/update.manifest\n" {
		t.Fatalf("arguments=%q, error=%v", args, err)
	}
}
