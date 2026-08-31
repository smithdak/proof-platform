# AXP-E0001 Ownership Matrix

| Surface/path | Wave | Single writer | Reviewer | Integration owner |
|---|---|---|---|---|
| `contracts/**`, `evals/release-manager-live-v1.json` | W1 | E0001-01 contract steward | product owner at Gate B | orchestrator |
| `crates/proof-kernel/**` | W2 | E0001-06 kernel worker | contract/security reviewer | orchestrator |
| `crates/proof-storage/**` | W2 | E0001-07 storage worker | migration/security reviewer | orchestrator |
| `crates/proof-agent-runtime/**` | W3 | E0001-02 runtime worker | runtime reviewer | orchestrator |
| `crates/proof-content/**`, `registry/content/**` | W3 | E0001-03 preview worker | content/contract reviewer | orchestrator |
| `crates/proof-transport-cli/**` | W4 | E0001-08 CLI worker | credential-boundary reviewer | orchestrator |
| `crates/proof-agent-runtime/**` | W5 | E0001-10 runtime check worker | runtime/contract reviewer | orchestrator |
| `crates/proof-agent-runtime/**` | W6 | E0001-11 bootstrap-recovery worker | runtime/recovery reviewer | orchestrator |
| `crates/proof-storage/**` | W6 | E0001-12 feasibility worker | storage/security reviewer | orchestrator |
| `crates/proof-storage/**` | W7 | E0001-13 trusted-open worker | storage/security reviewer | orchestrator |
| `crates/proof-transport-cli/**` | W8 | E0001-09 live-preparation worker | independent workflow/security reviewer | orchestrator |
| `crates/proof-transport-cli/src/commands/delegation.rs` | W9 | E0001-14 orchestrator | host-context CLI verifier | orchestrator |
| CLI transfer command and transfer tests | W10 | E0001-15 orchestrator | host-context CLI verifier | orchestrator |
| CLI archive extraction, secure filesystem helper, and tests | W11 | E0001-16 orchestrator | host-context CLI verifier | orchestrator |
| `docs/dogfood/release-manager-live.md` | W12 | E0001-04 live verifier | product owner | orchestrator |
| root manifests/lockfile and edition release records | W13 | E0001-05 orchestrator | product owner | orchestrator |

No task may write an unowned surface. Cross-crate requirements go to the named
owner and are integrated only after that owner quiesces. The live verifier must
not be an E0001-02, E0001-03, E0001-06, E0001-07, E0001-08, E0001-09, or
E0001-10/E0001-11/E0001-12/E0001-13/E0001-14/E0001-15/E0001-16 author.
