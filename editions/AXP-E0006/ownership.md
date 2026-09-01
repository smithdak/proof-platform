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
| Credential-free launcher-first diagnostic and edition records | W6 | E0006-07 orchestrator | assignment validator | orchestrator |
| Redacted launcher-first product evidence | W7 | E0006-08 e0006-verifier | orchestrator | orchestrator |
| E0006-08 unique handoff | W7 | E0006-08 e0006-verifier | orchestrator | orchestrator |
| `proof-transport-cli` decision-intent UI and tests | W8 | E0006-09 e0006-security | E0006-10 independent verifier | orchestrator |
| E0006-09 unique handoff | W8 | E0006-09 e0006-security | orchestrator | orchestrator |
| Intent-guard review and redacted evidence | W9 | E0006-10 e0006-verifier | orchestrator | orchestrator |
| E0006-10 unique handoff | W9 | E0006-10 e0006-verifier | orchestrator | orchestrator |
| Future intent-bound product evidence | W10 | E0006-11 e0006-verifier | orchestrator | orchestrator |
| E0006-11 unique handoff | W10 | E0006-11 e0006-verifier | orchestrator | orchestrator |
| Public guidance, contract reconciliation, root manifests/lockfile, and release records | W11 | E0006-04 orchestrator | product owner at Gate C | orchestrator |

No task may write an unowned surface. E0006-02 and E0006-05 use the same named
CLI security owner in disjoint waves; no other task may edit that crate.
E0006-03 and E0006-06 use the same distinct non-author verifier in disjoint
waves; E0006-08 reused that verifier under D-E0006-018 and is now quiescent
after the blocked result recorded in D-E0006-019. Root
manifests, lockfiles, final contract status, public
guidance, and release records remain orchestrator-owned. The orchestrator
creates new task and handoff placeholders before dispatch; handoff contents
then become exclusive to the named task in its active wave.

E0006-09 reuses the named CLI security owner only after E0006-08 is quiescent.
E0006-10 and E0006-11 reused the distinct verifier in disjoint waves. E0006-11
is blocked and quiescent after D-E0006-023 was consumed at the launcher gate.
