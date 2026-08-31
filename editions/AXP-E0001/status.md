# E0001 Status

- Edition: `AXP-E0001`
- Last updated: 2026-08-30
- Overall: `active`
- Current wave/task: W10 / E0001-04 live dogfood, owner-deferred until the morning of 2026-08-31 before provider construction
- Owner action needed on 2026-08-31: make `OPENAI_API_KEY` available through the secure agent environment; do not put the value in chat, Git, an edition record, or the workspace

## Gates

- [x] Gate A — direction approved 2026-08-30
- [x] Gate B — B1-B4/B6 approved 2026-08-30
- [x] Gate B — B5 conditionally approved 2026-08-30; activation evidence passes, with E0001-14 host CLI/readiness replay retained as a pre-provider entry check
- [x] Gate B clarification — D-E0001-009 narrows E0001 to a trusted fresh local workspace after E0001-12 proved strong stock-SQLite descriptor pinning impossible
- [ ] Human approval — exact preview consequence signed during live run
- [ ] Gate C — release accepted

## Discovery results

- [x] Runtime/provider continuation and recovery audit
- [x] Preview adapter and release-contract audit
- [x] Live dogfood, secret, budget, and independent-verification audit
- [x] Official OpenAI Responses continuation boundary checked
- [x] Initial eight-task W1-W6 dependency graph and exclusive paths validated
- [x] Edition validation passes before Gate A
- [x] E0001-01 contract/policy repaired through two audit cycles
- [x] Final independent Gate B re-audit passed
- [x] W2 kernel/storage implementation independently audited with no blocking findings
- [x] W2 scoped impact gates passed: kernel 419 tests; storage 206 tests
- [x] E0001-03 Content implementation passed final independent audit and 42 scoped crate tests
- [x] E0001-02 Runtime implementation passed final recovery/security audits and 96 scoped crate tests
- [x] W3 reverse-impact gates passed after final SQLite repairs: Runtime plus CLI 135 tests; Content 341 tests
- [x] E0001-08 CLI implementation passed independent credential/authority audit, 39 scoped tests, and 39-test reverse-impact gate
- [x] Interim nine-task W1-W7 dependency graph validated before the runtime check-only API audit
- [x] Ten-task W1-W8 graph validated before bootstrap/storage audit
- [x] E0001-10 check-only runtime API passed 99 tests, root rerun, and independent contract audit
- [x] Twelve-task W1-W9 graph validated before the stock-SQLite feasibility stop
- [x] Revised thirteen-task W1-W10 graph with E0001-12 negative evidence and E0001-13 trusted-open repair validates
- [x] E0001-09 public fresh preparation passed independent verification: stable immutable replay, exact ordered 10/10, all nine static live digests, exact authority/edition/next-argv bindings, and zero provider/live-v2/artifact/failure evidence
- [x] Final quiescent reverse-impact reruns passed after E0001-09: Runtime plus CLI 178 tests; Storage plus CLI/HTTP/MCP/WebSocket 259 tests; CLI 68 tests
- [x] E0001-14 repaired CLI delegation principal-kind mutation: focused delegation tests 5/5, all test targets compile, and format/diff checks pass
- [x] Revised fourteen-task W1-W11 graph preserves one CLI writer and makes host-context CLI/readiness replay an explicit pre-provider W10 barrier

## Risks and next actions

| Risk/blocker | Impact | Owner | Next action/date |
|---|---|---|---|
| `release.publish` v1 lacks the approved identity/replay behavior | resolved in W3; v1 remains compatible and v2 is exact | E0001-03 | final independent audit passed; reverse-impact gate runs after W3 quiesces |
| Provider completion can be ambiguous before local response checkpoint | resolved in W3 with strict durable attempt/event barriers and restart tests | E0001-02 | both independent audits and Runtime reverse-impact gate passed |
| Current release result is random, local, and not durably bound to a requested edition | resolved in W3 with immutable, independently verified preview artifact | E0001-03 | final independent audit passed; reverse-impact gate runs after W3 quiesces |
| Provider usage has tokens but no `cost_microusd` | resolved locally with nullable provider cost plus sealed conservative calculated cost | E0001-02/product owner | E0001-09 now passes; enforce the 120000 micro-USD runtime ceiling and preserve null provider cost as unavailable during E0001-04 |
| Live call sends data to a paid external provider | high; credential/data/external effect | product owner | B5 evidence prerequisites pass; retain the exact direct `gpt-5.6-sol`, one-run, USD 0.15, synthetic-only boundary |
| CLI delegation SQL omitted v12 `scope_json` | resolved in E0001-08 with exact save/load/list/validate/transfer scope round trips | E0001-08 | independent credential/authority audit and reverse-impact gate passed |
| CLI delegation grant could reclassify the workspace Agent as Human for a distinct recipient, or an enrolled Human recipient as Agent | resolved in E0001-14; issuer uses its actual keypair principal and existing recipient identity is immutable | E0001-14 | focused 5/5 and compile/format clean; retained packet independently remains Agent requester plus Human approver; rerun full CLI in host context before live start |
| CLI read `OPENAI_API_KEY` before deterministic and authority checks | resolved in E0001-08 with the audited deferred factory seam and zero-call failure spies | E0001-08 | direct live factory remains the only live-command reader at the credential boundary |
| Runtime authoritative live setup/target-agent validator was private | resolved by E0001-10 public read-only exact-validator path | E0001-10 | 99 tests and independent audit passed; CLI integration remains under E0001-09 |
| Generic runtime start can die after run save but before checkpoint/Started | resolved and independently audited in E0001-11 | E0001-11 | owner/root runtime suites pass 110/110; focused adversarial recovery 11/11; final audit PASS |
| Strong descriptor-pinned SQLite main/WAL/sidecar semantics | impossible with stock Unix VFS; `/proc/self/fd` conflicts with native nofollow and canonicalizes without it | E0001-12/D-E0001-009 | preserve negative evidence; do not claim the impossible guarantee or add a custom VFS in E0001 |
| Path-only SQLite open follows a storage/database symlink before CLI secure checks | resolved within D-E0001-009's explicit fresh-workspace threat boundary | E0001-13/E0001-09 | storage 124/124 and independent PASS; CLI 68/68, 11 child tests, and independent PASS |
| No public command can create fresh terminal 10/10 evidence plus a safe synthetic edition without credentials | resolved | E0001-09 | fresh public packet independently verified exactly 10/10 with stable replay and zero provider attempts |
| Host process has no `OPENAI_API_KEY` | blocks the sole approved paid request before provider construction; no charge or external effect occurred | product owner/operator | owner-deferred until the morning of 2026-08-31; then inject the credential through the secure agent environment and resume only the persisted exact `next_argv` |
| Fresh readiness workspace is under `/tmp` | host cleanup or reboot could remove the retained packet before morning | orchestrator/operator | preserve the workspace overnight; if it disappears, stop and regenerate/reverify credential-free readiness before any paid attempt |
| Historical E0000 workspace identity is compromised | critical if reused | orchestrator | use only a fresh temporary workspace and newly generated test identities |
