#!/usr/bin/env bash
# Run tests for a crate and its dependents only. Usage: scripts/test-scoped.sh <crate-name>
set -euo pipefail

crate="${1:?usage: test-scoped.sh <crate-name>}"

case "$crate" in
  proof-kernel)        packages="-p proof-kernel" ;;
  proof-content)       packages="-p proof-content -p proof-transport-http -p proof-transport-mcp -p proof-transport-cli" ;;
  proof-commerce)      packages="-p proof-commerce" ;;
  proof-storage)       packages="-p proof-storage -p proof-kernel -p proof-transport-http -p proof-transport-cli" ;;
  proof-transport-http) packages="-p proof-transport-http" ;;
  proof-transport-mcp) packages="-p proof-transport-mcp" ;;
  proof-transport-cli) packages="-p proof-transport-cli" ;;
  *) echo "unknown crate: $crate" >&2; exit 1 ;;
esac

echo "==> testing: $packages"
rtk cargo test $packages
