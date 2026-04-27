#!/usr/bin/env bash
set -euo pipefail
os="$(uname -s)"
arch="$(uname -m)"
case "$os:$arch" in
  Linux:x86_64) echo linux-x86_64 ;;
  Darwin:arm64|Darwin:aarch64) echo darwin-arm64 ;;
  *) echo "unsupported bench profile for $os/$arch" >&2; exit 1 ;;
esac
