#!/usr/bin/env bash
# Swarm wave lifecycle helper for the orchestrating agent.
#
# Usage:
#   scripts/swarm.sh plan <wave-number> "<focus>"       # scaffold a wave task file
#   scripts/swarm.sh verify                             # pre-integration full check (orchestrator only)
#   scripts/swarm.sh commit <wave-number> "<message>"   # commit after green full suite
#
# Wave task files live in .swarm/wave-<n>/tasks.md and define:
#   - crate ownership partitioning per agent
#   - interface changes proposed (must update contracts/kernel-api.md)
#   - scoped test command per agent
set -euo pipefail

cmd="${1:-}"
case "$cmd" in
  plan)
    wave="${2:?wave number required}"
    focus="${3:-}"
    dir=".swarm/wave-${wave}"
    mkdir -p "$dir"
    cat > "$dir/tasks.md" <<EOF
# Wave ${wave}: ${focus}

## Crate Ownership (disjoint — no overlaps)

| Agent | Crates | Scoped test command |
|---|---|---|
| (assign) | | scripts/test-scoped.sh <crate> |

## Interface Changes Proposed

- (none | describe each shared-type change and the contract update needed)

## Integration Checklist (orchestrator)

- [ ] contracts/kernel-api.md updated if shared types changed
- [ ] each agent's scoped tests green
- [ ] full workspace suite green (orchestrator runs scripts/swarm.sh verify)
- [ ] conflicts resolved in favor of the interface contract
EOF
    echo "scaffolded $dir/tasks.md"
    ;;

  verify)
    echo "==> formatting check"
    cargo fmt --check --all
    echo "==> full workspace test"
    cargo test --workspace
    ;;

  commit)
    wave="${2:?wave number required}"
    msg="${3:?message required}"
    git add -A
    git commit -m "$msg"
    echo "committed wave ${wave}"
    ;;

  *)
    echo "usage: swarm.sh {plan|verify|commit} ..." >&2
    exit 1
    ;;
esac
