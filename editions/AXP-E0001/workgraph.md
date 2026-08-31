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
| E0001-14 | W9 | orchestrator / `gpt-5.6-sol` | CLI delegation grant and unique handoff | E0001-09 | Preserve immutable workspace/recipient principal identity across distinct-recipient grants; reject kind collisions | done; focused 5/5 and compile/format clean; current host entry gate passes |
| E0001-15 | W10 | orchestrator / `gpt-5.6-sol` | CLI workspace import and unique handoff | E0001-14 | Reject principal replacement, delegation replacement/unrevocation, and invalid-proof partial identity persistence before archive writes | done; focused 3/3 plus compatibility, compile, format, host 72/72, and readiness replay clean |
| E0001-16 | W11 | orchestrator / `gpt-5.6-sol` | CLI archive extraction/secure writes and unique handoff | E0001-15 | One archive snapshot; reject traversal/symlink/proof drift before persistence; descriptor-relative atomic JSON replacement | done; adversarial 5/5, compatibility 4/4, compile/format, host 78/78, and exact readiness replay clean |
| E0001-17 | W12 | orchestrator / `gpt-5.6-sol` | CLI workspace identity lifecycle, README, and unique handoff | E0001-16 | Exact config-actor/key binding; no linked config, destructive reinit, pre-validation rotation mutation, or archive collision | done; lifecycle 4/4, compile/format, host 81/81, and exact readiness replay clean |
| E0001-18 | W13 | orchestrator / `gpt-5.6-sol` | canonical kernel/domain/live contracts and unique handoff | E0001-17 | Canonical docs reflect active handler hooks, SQLite v12, active release.publish::v2, and activated-but-unexecuted B5 | done; exact cross-check, host 81/81, and readiness replay clean |
| E0001-19 | W14 | orchestrator / `gpt-5.6-sol` | Runtime/CLI live-v2 approval visibility and unique handoff | E0001-02, E0001-08, E0001-18 | Event-independent typed watch; event-bound actionable review; exact request/step/call/Human; no generic v2 resume | done; Runtime 116/116, host CLI 88/88, real-SQLite/adversarial regressions, independent PASS, formatting/diff, and readiness replay clean |
| E0001-20 | W15 | orchestrator / `gpt-5.6-sol` | Kernel/Storage/Runtime/CLI start claim and emitted recovery; secure runbook | E0001-02, E0001-07..09, E0001-18, E0001-19 | Atomic four-record claim; exact replay no-dispatch; pristine same-run recovery; workspace/policy/Human argv; secure operator procedure | done; Kernel 98/98, Storage 128/128, Runtime 119/119, host CLI 93/93, reverse impact 597/597, independent PASS, and readiness replay clean |
| E0001-04 | W16 | unassigned / `gpt-5.6-sol` | live dogfood record and unique handoff | E0001-02, E0001-03, E0001-06..20 + paid-use Gate B | Final-source host CLI, immutable readiness replay, fresh live run, signed approval, independent 17/17 verification, cost/rollback evidence | owner-deferred until credential availability; zero provider attempts |
| E0001-05 | W17 | orchestrator / `gpt-5.6-sol` | integration and edition release records | E0001-01..04, E0001-06..20 | Quiescent workspace gate and dated Gate C decision | blocked |

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
  |                                                                                         v
  |                                                                                E0001-15 import repair
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-16 archive repair
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-17 identity binding
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-18 contract sync
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-19 review visibility
  |                                                                                         |
  |                                                                                         v
  |                                                                                E0001-20 one-shot start
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
- W10 closes the adjacent credential-free workspace-import authority hazards.
  It must preflight immutable principal identity, exact existing delegations,
  and every proof signature before archive identity/proof writes, while
  preserving compatible imports and the retained packet.
- W11 closes archive extraction ambiguity and filesystem escape. It must use
  one snapshot, reject unsafe/duplicate entries and unbound proof files before
  persistence, and atomically replace contained JSON without following links.
- W12 closes the ordinary workspace identity lifecycle before any paid call.
  Config actor and signing key must match on every open and before rotation;
  initialization and rotated archives must never replace an identity silently.
- W13 synchronizes the canonical contracts with the approved and implemented
  E0001 surface. It changes no code or normative API and must preserve the
  difference between active prerequisites and the still-pending live Gate C.
- W14 closes the live-v2 review visibility gap without entering a provider
  boundary. Diagnostic watch accepts valid typed crash-window checkpoints;
  actionable approval additionally requires exact immutable event, request,
  step, call, argument, and Human bindings. Runtime 116/116, host CLI 88/88,
  formatting/diff, two independent reviews, and exact readiness replay pass.
- W15 makes live start globally one-shot before entering a provider boundary.
  Kernel/Storage commit one canonical claim plus its initial run/checkpoint/
  Started records atomically; Runtime returns exact replay without dispatch and
  recovers only an exact pristine same run; CLI emits workspace/policy/Human-
  bound follow-ups; the secure runbook prohibits reconstructed argv, generic
  resume, and approval UI. Final reverse impact passes 597/597 across 49
  suites, all independent audits pass after one runbook correction, and exact
  retained readiness replay remains 10/10.
- W16 starts only after deterministic preflight is independently 10/10, the
  frozen 17-check live policy is validated, and the owner has explicitly
  approved credential, provider/model, spend, synthetic data, evidence
  retention, and the human approver. Those policy/evidence prerequisites and
  the final-source E0001-20 host/readiness gates pass. Execution remains
  stopped until `OPENAI_API_KEY` is securely available to that host process.
- W17 starts only after all writers quiesce. The orchestrator reconciles root
  manifests, runs impact plus final workspace verification, and requests Gate C.
