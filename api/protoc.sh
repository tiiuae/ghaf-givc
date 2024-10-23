#!/usr/bin/env bash

gen_protoc() {
    protoc  --proto_path="$1" \
            --go_out="$1" --go_opt=paths=source_relative \
            --go-grpc_out="$1" --go-grpc_opt=paths=source_relative \
            "$1"/"$2"
}

if [ $# -eq 0 ]; then
    set -- admin hwid locale systemd wifi
fi

for protob in "$@"; do
	gen_protoc api/"$protob" "$protob".proto
done
