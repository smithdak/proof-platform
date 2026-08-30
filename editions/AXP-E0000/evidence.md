# E0000 Evidence Register

Attach durable, reviewable evidence here; summaries alone do not close work.

| Item | Evidence required | Location / result | Reviewer | Status |
|---|---|---|---|---|
| E0000-01 | Tracked artifact and assignment protocol check | [`handoffs/E0000-01.md`](handoffs/E0000-01.md) | orchestrator | passed |
| E0000-02 | Owner approval, non-content exposure audit, permissions and remediation record; files preserved | [`handoffs/E0000-02.md`](handoffs/E0000-02.md) | security owner | passed with residual P0 rotation risk |
| E0000-03 | Swarm validation with three disjoint workers and overlap rejection | [`handoffs/E0000-03.md`](handoffs/E0000-03.md) | orchestrator | passed |
| E0000-04 | Reverse dependency report and scoped-selection output | [`handoffs/E0000-04.md`](handoffs/E0000-04.md) | test steward | passed |
| E0000-05 | Contract diff, decision entry, registry/code reconciliation | Approved freeze and mismatch resolution in [`handoffs/E0000-05.md`](handoffs/E0000-05.md) | contract steward | passed |
| E0000-13 | Shared replay design, migration/API proposal, atomicity and test plan | [`handoffs/E0000-13.md`](handoffs/E0000-13.md) | replay design reviewer | passed; D-E0000-006 approved |
| E0000-14 | Kernel exact replay API/engine implementation | [`handoffs/E0000-14.md`](handoffs/E0000-14.md) — 91 kernel tests; 385-test reverse-dependency gate | kernel reviewer | passed |
| E0000-16 | SQLite migration 11 and replay-ledger round trips | [`handoffs/E0000-16.md`](handoffs/E0000-16.md) — 104 scoped tests passed; in-memory/file FK parity | storage reviewer | passed |
| E0000-15 | HTTP/WebSocket idempotency error mappings and tests | [`handoffs/E0000-15.md`](handoffs/E0000-15.md) — 26 HTTP and 9 WebSocket tests passed | transport reviewer | passed |
| E0000-18 | Canonical kernel API replay/migration reconciliation | [`handoffs/E0000-18.md`](handoffs/E0000-18.md) — reviewed paths/types, migration 11, replay lifecycle, and downstream deviations | contract reviewer | passed |
| E0000-06 | Content handler, registry, schema, proof, and idempotency tests for two operations | [`handoffs/E0000-06.md`](handoffs/E0000-06.md) — 29 scoped tests; independent review repaired initialized-workspace registry lookup; final seven-package impact gate passed 231 tests across 31 suites after transport and conformance integration | orchestrator | passed |
| E0000-10 | CLI governed-engine and proof-persistence tests | [`handoffs/E0000-10.md`](handoffs/E0000-10.md) — 25 scoped tests; workspace signer/store, original proof replay, conflict and proof-free local helper independently reviewed | orchestrator | passed |
| E0000-11 | HTTP discovery, execution, error, and proof tests | [`handoffs/E0000-11.md`](handoffs/E0000-11.md) — 31 scoped tests; tower discovery/replay/conflict, durable original proof, and canonical 403/409 mappings independently reviewed | orchestrator | passed |
| E0000-12 | MCP schema, run-metadata, execution, and proof tests | [`handoffs/E0000-12.md`](handoffs/E0000-12.md) — 25 scoped tests; shared replay/approval/run store, original proof, exact retry, conflict, and 21-operation registry independently reviewed | orchestrator | passed |
| E0000-17 | WebSocket discovery, execution, error, and exact-proof replay tests | [`handoffs/E0000-17.md`](handoffs/E0000-17.md) — 11 scoped tests over real loopback WebSockets; same signer/store, original durable proofs for both operations, exact retries, mutation counts, conflicts, and stable error codes independently reviewed | orchestrator | passed |
| E0000-07 | Executable eight-operation conformance output | [`handoffs/E0000-07.md`](handoffs/E0000-07.md) — 6 locked scoped tests; JSON drives all eight engine calls, three signed approvals, eight scoped delegations/original proofs, and exact replay/conflict only for the frozen pair | orchestrator | passed |
| E0000-08 | Repeated deterministic trace digest + 10/10 evaluation | [`handoffs/E0000-08.md`](handoffs/E0000-08.md) — 38 scoped tests; two independent fixed traces directly consume the checked-in policy, pass all 10 checks, include approval resume/no failures, and pin trace digest `b14a94ed…ae34` | orchestrator | passed |
| E0000-09 | Scoped verification and owner release decision | [`handoffs/E0000-09.md`](handoffs/E0000-09.md) — final quiescent gate passed 405 tests across 46 suites; residual risks recorded; Gate C approved 2026-08-29 | product owner | passed |

Never store private key contents, credentials, or copied workspace databases in
this register.

Worker details live in unique files under [`handoffs/`](handoffs/). The
orchestrator copies only accepted, non-sensitive evidence into this register.
