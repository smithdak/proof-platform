# AXP-E0001 Workgraph

Edition: `AXP-E0001`

| Task | Wave | Owner/model | Owned paths | Depends on | Acceptance | Status |
|---|---|---|---|---|---|---|
| E0001-01 | W1 | e0001-contract-steward / `gpt-5.6-sol` | contracts, new sealed eval policy | none | Audit-repaired, owner-ready contract/recovery/cost packet | done |
| E0001-06 | W2 | e0001-kernel-owner / `gpt-5.6-sol` | `proof-kernel` | E0001-01 + Gate B | Default-compatible version-aware handler API and authority regressions | done |
| E0001-07 | W2 | e0001-storage-owner / `gpt-5.6-sol` | `proof-storage` | E0001-01 + Gate B | Next migration plus scoped-delegation round trip and engine-load support | done |
| E0001-02 | W3 | e0001-runtime-finish / `gpt-5.6-sol` | `proof-agent-runtime` | E0001-06, E0001-07 | Durable attempt/recovery/evaluator tests; runtime impact green | done |
| E0001-03 | W3 | e0001-content-owner / `gpt-5.6-terra` | `proof-content`, Content registry | E0001-06, E0001-07 | Durable preview, exact binding/replay/proof tests; Content impact green | done |
| E0001-08 | W4 | e0001-cli-owner / `gpt-5.6-sol` | `proof-transport-cli` | E0001-02, E0001-03, E0001-06, E0001-07 | Frozen 17-check validation before secret read; exact scoped-delegation and live-command round trips | done |
| E0001-10 | W5 | e0001-runtime-check / `gpt-5.6-sol` | `proof-agent-runtime` | E0001-02, E0001-08 | Public authoritative check-only live-start validation with zero mutation/factory calls | done |
| E0001-11 | W6 | e0001-runtime-recovery / `gpt-5.6-sol` | `proof-agent-runtime` | E0001-02, E0001-10 | Exact generic start-bootstrap reconciliation through public resume | done |
| E0001-12 | W6 | e0001-storage-secure / `gpt-5.6-sol` | `proof-storage` | E0001-07, E0001-10 | Reproduce stock-SQLite descriptor-pinning infeasibility and stop without a false guard | done: negative feasibility result |
| E0001-13 | W7 | e0001-storage-secure / `gpt-5.6-sol` | `proof-storage` | E0001-07, E0001-10, E0001-12 | Existing-DB native nofollow open with descriptor/inode barriers under D-E0001-009 | done |
| E0001-09 | W8 | e0001-cli-owner / `gpt-5.6-sol` | `proof-transport-cli` | E0001-08, E0001-10, E0001-11, E0001-13 | Public credential-free runtime rehearsal, immutable fresh 10/10 evidence, safe synthetic edition, exact readiness packet | done |
| E0001-14 | W9 | orchestrator / `gpt-5.6-sol` | CLI delegation grant and unique handoff | E0001-09 | Preserve immutable workspace/recipient principal identity across distinct-recipient grants; reject kind collisions | done; focused 5/5 and compile/format clean; host full-suite replay is a W10 entry check |
| E0001-04 | W10 | unassigned / `gpt-5.6-sol` | live dogfood record and unique handoff | E0001-02, E0001-03, E0001-06..14 + paid-use Gate B | Host CLI replay, immutable readiness replay, fresh live run, signed approval, independent 17/17 verification, cost/rollback evidence | owner-deferred until morning 2026-08-31; host credential unavailable; zero provider attempts |
| E0001-05 | W11 | orchestrator / `gpt-5.6-sol` | integration and edition release records | E0001-01..04, E0001-06..14 | Quiescent workspace gate and dated Gate C decision | blocked |

## Dependency flow

```text
Gate A
  |
  v
E0001-01 contract + recovery/evaluation design
  |
  +--> Gate B: public contract/runtime risk decision
  |       |
  |       +--> E0001-06 kernel version/authority --+
  |       +--> E0001-07 scoped delegation storage -+--> E0001-02 runtime recovery --+
  |                                                +--> E0001-03 local preview -----+--> E0001-08 CLI/live boundary
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-10 runtime check API
  |                                                                                         |
  |                                                                                         v
  |                                                                         +------ E0001-11 bootstrap recovery
  |                                                                         +------ E0001-12 impossibility proof
  |                                                                                         |
  |                                                                                         v
  |                                                                         +------ E0001-13 trusted SQLite open
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-09 live preparation
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-14 identity repair
  |                                                                                         |
  +--> Gate B: credential/model/spend approval                                              v
                                                                                   E0001-04 live gate
                                                                                            |
                                                                                            v
                                                                                   E0001-05 / Gate C
```

## Wave gates

- W1 starts only after Gate A and produces design/evidence, not live calls.
- W2 starts only after the owner approves the repaired E0001-01 Gate B packet.
  Its kernel and storage writers are crate-disjoint.
- W3 starts after W2 passes. Runtime and Content writers are crate-disjoint;
  neither may use a credential or provider.
- W4 integrates the quiescent W2/W3 APIs in the CLI, proves local authority
  and secret ordering, and still may not use a credential or provider.
- W5 exposes the runtime's existing authoritative start validation through one
  public read-only API, with no gateway invocation or live-run mutation.
- W6 closes the original audit-proven prerequisite seams. E0001-11 reconciles
  runtime bootstrap and E0001-12 preserves the negative stock-SQLite
  feasibility result.
- W7 follows the E0001-12 stop and D-E0001-009 decision with E0001-13's
  narrowly trusted fresh-workspace SQLite open.
- W8 adds the missing public credential-free preparer. It must produce fresh,
  immutable 10/10 evidence and exact readiness bindings through real runtime,
  approval, evaluator, storage, and setup paths without a live provider.
- W9 closes the credential-free delegation principal-kind regression found
  during final operator audit. It must preserve the workspace Agent identity,
  reject an existing recipient of another kind, and leave the retained packet
  untouched.
- W10 starts only after deterministic preflight is independently 10/10, the
  frozen 17-check live policy is validated, and the owner has explicitly
  approved credential, provider/model, spend, synthetic data, evidence
  retention, and the human approver. Those policy/evidence prerequisites now
  pass. Before provider construction, host context must replay the full CLI
  suite and the immutable readiness packet after E0001-14. Execution remains
  stopped until `OPENAI_API_KEY` is securely available to that host process.
- W11 starts only after all writers quiesce. The orchestrator reconciles root
  manifests, runs impact plus final workspace verification, and requests Gate C.
