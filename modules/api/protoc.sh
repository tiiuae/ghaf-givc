#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0


gen_protoc() {
    protoc  --proto_path="api" \
            --go_out="$1" --go_opt=paths=source_relative \
            --go-grpc_out="$1" --go-grpc_opt=paths=source_relative \
            "$2"
}

if [ $# -eq 0 ]; then
    set -- admin systemd socket stats hwid locale wifi event notify policyagent
fi

for protodir in "$@"; do
    for protobuf in api/"$protodir"/*.proto; do
        gen_protoc modules/api "$protobuf"
    done
done
