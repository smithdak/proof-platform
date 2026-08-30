# E0000 Workgraph

The graph is intentionally bounded so cheaper agents can take leaf work.

Detailed assignment packets live in `editions/AXP-E0000/tasks/`:

- [E0000-01](tasks/E0000-01.md) records and status protocol
- [E0000-02](tasks/E0000-02.md) owner-gated `.proof` security remediation
- [E0000-03](tasks/E0000-03.md) swarm launcher ownership checks
- [E0000-04](tasks/E0000-04.md) reverse-dependency test impact
- [E0000-05](tasks/E0000-05.md) contract drift reconciliation
- [E0000-13](tasks/E0000-13.md) shared exact-replay design
- [E0000-14](tasks/E0000-14.md) kernel replay API and engine implementation
- [E0000-16](tasks/E0000-16.md) SQLite replay ledger and migration
- [E0000-15](tasks/E0000-15.md) HTTP/WebSocket idempotency error mapping
- [E0000-18](tasks/E0000-18.md) kernel contract reconciliation
- [E0000-06](tasks/E0000-06.md) governed content and registry operations
- [E0000-10](tasks/E0000-10.md) governed CLI path
- [E0000-11](tasks/E0000-11.md) governed HTTP path
- [E0000-12](tasks/E0000-12.md) governed MCP path
- [E0000-17](tasks/E0000-17.md) governed WebSocket path
- [E0000-07](tasks/E0000-07.md) executable conformance
- [E0000-08](tasks/E0000-08.md) deterministic Release Manager automation
- [E0000-09](tasks/E0000-09.md) integration and owner release gate

```text
E0000-01 records
  ├── E0000-02 security remediation (owner-approved P0)
  ├── E0000-03 swarm launcher
  ├── E0000-04 test impact
  └── E0000-05 contract drift
          └── E0000-13 shared replay design
                  ├── E0000-14 kernel replay API + engine
                  ├── E0000-16 SQLite replay ledger + migration
                  └── E0000-15 HTTP + WebSocket error mapping
                          └── E0000-18 canonical kernel contract
                                  └── E0000-06 content + registry
                                          ├── E0000-10 CLI
                                          ├── E0000-11 HTTP
                                          ├── E0000-12 MCP
                                          └── E0000-17 WebSocket
E0000-10 + E0000-11 + E0000-12 + E0000-17
  └── E0000-07 executable conformance
          └── E0000-08 deterministic release manager
E0000-02..08 + E0000-10..18 ──> E0000-09 integration / owner gate
```

The three W4 replay tasks complete and the approved interface is reconciled
into the canonical kernel contract before content/registry. CLI, HTTP, and MCP
then fan out concurrently; WebSocket follows as its own bounded packet so every
peer proves exact original-proof replay. Conformance and runtime evidence remain
dependency-ordered after all four transport integrations.

## Assignment packet template

```text
Edition/item:
Objective:
Owned paths (exclusive):
Read-only references:
Dependencies:
Non-goals:
Model tier and budget:
Acceptance command/check:
Handoff: files, tests, assumptions, risks, follow-up requests
```

## Wave policy

The orchestrator may run three workers concurrently only when their exclusive
paths do not overlap. `assignments.tsv` is the machine-checked dispatch record;
task packets define acceptance and handoff details. A worker needing a shared
path submits a request. The orchestrator quiesces all writers before
integration and full edition review. No worker commits or changes root
manifests, lockfiles, migration numbering, or another worker's crate.
