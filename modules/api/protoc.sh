#!/usr/bin/env bash

gen_protoc() {
    protoc  --proto_path="$1" \
            --go_out="$2" --go_opt=paths=source_relative \
            --go-grpc_out="$2" --go-grpc_opt=paths=source_relative \
            "$1"/"$3"
}

if [ $# -eq 0 ]; then
    set -- admin systemd socket hwid locale wifi
fi

for protob in "$@"; do
	gen_protoc api/"$protob" modules/api/"$protob" "$protob".proto
done
