# E0000 Ownership Matrix

| Surface | Owner | Worker limit | Rule |
|---|---|---:|---|
| `editions/AXP-E0000/**`, `editions/BACKLOG.md` | edition orchestrator | 1 | Workers write only their unique handoff while active; orchestrator owns shared records |
| `.proof/**`, `.gitignore` | security owner approved by product owner | 1 | Preserve files; no delete; record rotation/quarantine decision |
| `scripts/swarm.sh`, `scripts/swarm-fixtures/**` | process worker | 1 | No root or crate edits |
| `scripts/test-scoped.sh` | test-impact worker | 1 | Must derive and report workspace reverse dependents |
| `contracts/**`, `ARCHITECTURE.md` | contract steward | 1 | Update only with decision entry |
| `crates/proof-kernel/**` replay API/engine | kernel replay owner | 1 | Public interface is frozen by D-E0000-006 before fan-out |
| `crates/proof-storage/**` replay ledger/migration | storage replay owner | 1 | Sole migration-number and SQLite implementation owner |
| HTTP/WS idempotency error mapping | replay transport owner | 1 | Owns only `proof-transport-http` and `proof-transport-ws` in W4 |
| `crates/proof-content/**`, `registry/content/**`, `schemas/content/**` | content worker | 1 | Owns domain/registry behavior; requests transport changes |
| `crates/proof-transport-cli/**` | CLI worker | 1 | Starts after content integration |
| `crates/proof-transport-http/**` | HTTP worker | 1 | Starts after content integration |
| `crates/proof-transport-mcp/**` | MCP worker | 1 | Starts after content integration |
| `crates/proof-transport-ws/**` | WebSocket worker | 1 | Starts after content integration; must return the engine's original replay proof |
| `conformance/**` | conformance worker | 1 | Depends on finalized contracts and transports |
| `crates/proof-agent-runtime/**`, `docs/dogfood/**`, `evals/**` | runtime evidence worker | 1 | Deterministic fixture only in E0000 |
| root manifests, lockfiles, integration | orchestrator | 1 | Integrate after writers quiesce |

Every assignment must list the exact subpaths before start. Shared types and
error mappings are coordinated requests, never opportunistic edits.

[`assignments.tsv`](assignments.tsv) is the dispatch source of truth and task
packets in [`tasks/`](tasks/) define each job. A worker may write only the paths
in its assignment. E0000-10, E0000-11, and E0000-12 may run together; E0000-17
follows before conformance. All other dependency edges remain sequential.
