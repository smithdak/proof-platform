# AXP-E0006 Ownership Matrix

| Surface/path | Wave | Single writer | Reviewer | Integration owner |
|---|---|---|---|---|
| Proposed session contract, evaluation, backlog, and edition scaffold | W1 | E0006-01 orchestrator | product owner at Gate A/B | orchestrator |
| Required E0006-02..04 handoff placeholders | W1 | E0006-01 orchestrator | assignment validator | orchestrator |
| `crates/proof-transport-cli/**` and exact `Cargo.lock` rustix delta | W2 | E0006-02 e0006-security | independent security reviewer | orchestrator |
| `docs/dogfood/approval-console-secure.md` | W3 | E0006-03 e0006-verifier | orchestrator | orchestrator |
| E0006-02 unique handoff | W2 | E0006-02 security owner | orchestrator | orchestrator |
| E0006-03 unique handoff | W3 | E0006-03 e0006-verifier | orchestrator | orchestrator |
| Public guidance, contract reconciliation, root manifests/lockfile, and release records | W4 | E0006-04 orchestrator | product owner at Gate C | orchestrator |

No task may write an unowned surface. E0006-02 e0006-security is the only CLI writer and
E0006-03 must be a distinct non-author. Root manifests, lockfiles, final
contract status, public guidance, and release records remain orchestrator-owned.
The W1 orchestrator creates later tasks' required handoff placeholders; their
contents become exclusive to the named task in that task's later wave.
