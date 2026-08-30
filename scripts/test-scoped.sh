#!/usr/bin/env bash
# Test one changed workspace package plus every reverse transitive workspace
# dependent. Usage: scripts/test-scoped.sh <package> [--list]
set -euo pipefail

package="${1:-}"
mode="${2:-}"

if [[ -z "$package" || ( -n "$mode" && "$mode" != "--list" ) ]]; then
  echo "usage: scripts/test-scoped.sh <package> [--list]" >&2
  exit 2
fi

workspace_packages=()
declare -A workspace_set=()
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  candidate="${line%% *}"
  workspace_packages+=("$candidate")
  workspace_set["$candidate"]=1
done < <(
  rtk cargo tree --workspace --depth 0 --prefix none
)

if [[ -z "${workspace_set[$package]:-}" ]]; then
  echo "unknown workspace package: $package" >&2
  exit 2
fi

declare -A impacted=()
while IFS= read -r line; do
  candidate="${line%% *}"
  if [[ -n "${workspace_set[$candidate]:-}" ]]; then
    impacted["$candidate"]=1
  fi
done < <(
  rtk cargo tree \
    --workspace \
    --invert "$package" \
    --prefix none \
    --edges normal,build,dev \
    --all-features
)

selected=()
package_args=()
for candidate in "${workspace_packages[@]}"; do
  if [[ -n "${impacted[$candidate]:-}" ]]; then
    selected+=("$candidate")
    package_args+=("-p" "$candidate")
  fi
done

if [[ ${#selected[@]} -eq 0 ]]; then
  echo "cargo did not return an impact set for: $package" >&2
  exit 1
fi

echo "changed package: $package"
echo "impacted packages (${#selected[@]}): ${selected[*]}"

if [[ "$mode" == "--list" ]]; then
  exit 0
fi

rtk cargo test "${package_args[@]}"
