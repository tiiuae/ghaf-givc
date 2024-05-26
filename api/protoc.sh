#!/usr/bin/env bash

gen_protoc() {
    protoc  --proto_path="$1" \
            --go_out="$1" --go_opt=paths=source_relative \
            --go-grpc_out="$1" --go-grpc_opt=paths=source_relative \
            "$1"/"$2"
}

gen_protoc api/admin admin.proto
gen_protoc api/systemd systemd.proto