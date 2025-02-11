#!/usr/bin/env bash

gen_protoc() {
    protoc  --proto_path="api" \
            --go_out="$1" --go_opt=paths=source_relative \
            --go-grpc_out="$1" --go-grpc_opt=paths=source_relative \
            "$2"
}

if [ $# -eq 0 ]; then
    set -- admin systemd socket stats hwid locale wifi
fi

for protodir in "$@"; do
    for protobuf in api/"$protodir"/*.proto; do
        gen_protoc modules/api "$protobuf"
    done
done
