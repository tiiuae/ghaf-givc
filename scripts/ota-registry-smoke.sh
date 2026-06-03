#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <manifest-path> <registry/repo:tag> [changelog-path]"
  exit 1
fi

manifest="$1"
reference="$2"
changelog="${3:-}"

push_args=(registry --insecure push --manifest "$manifest" "$reference")
if [[ -n "$changelog" ]]; then
  push_args+=(--changelog "$changelog")
fi

cargo run -p ota-update --bin ota-update -- "${push_args[@]}"
cargo run -p ota-update --bin ota-update -- registry --insecure pull "$reference" --destination "/tmp/ota-registry-smoke"
