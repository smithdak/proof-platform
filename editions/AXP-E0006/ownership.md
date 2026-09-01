# AXP-E0006 Ownership Matrix

| Surface/path | Wave | Single writer | Reviewer | Integration owner |
|---|---|---|---|---|
| Proposed session contract, evaluation, backlog, and edition scaffold | W1 | E0006-01 orchestrator | product owner at Gate A/B | orchestrator |
| Required E0006-02..04 handoff placeholders | W1 | E0006-01 orchestrator | assignment validator | orchestrator |
| `crates/proof-transport-cli/**` and exact `Cargo.lock` rustix delta | W2 | E0006-02 e0006-security | independent security reviewer | orchestrator |
| `docs/dogfood/approval-console-secure.md` | W3 | E0006-03 e0006-verifier | orchestrator | orchestrator |
| E0006-02 unique handoff | W2 | E0006-02 security owner | orchestrator | orchestrator |
| E0006-03 unique handoff | W3 | E0006-03 e0006-verifier | orchestrator | orchestrator |
| `crates/proof-transport-cli/**` remediation | W4 | E0006-05 e0006-security | E0006-06 independent security reviewer | orchestrator |
| E0006-05 unique handoff | W4 | E0006-05 security owner | orchestrator | orchestrator |
| Redacted browser/dogfood evidence | W5 | E0006-06 e0006-verifier | orchestrator | orchestrator |
| E0006-06 unique handoff | W5 | E0006-06 e0006-verifier | orchestrator | orchestrator |
| Public guidance, contract reconciliation, root manifests/lockfile, and release records | W6 | E0006-04 orchestrator | product owner at Gate C | orchestrator |

No task may write an unowned surface. E0006-02 and E0006-05 use the same named
CLI security owner in disjoint waves; no other task may edit that crate.
E0006-03 and E0006-06 use the same distinct non-author verifier in disjoint
waves. Root manifests, lockfiles, final contract status, public guidance, and
release records remain orchestrator-owned. The orchestrator creates new task
and handoff placeholders before dispatch; handoff contents then become
exclusive to the named task in its active wave.
