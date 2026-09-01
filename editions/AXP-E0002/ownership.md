# Ownership Matrix

| Surface/path | Wave | Single writer | Reviewer | Integration owner |
|---|---|---|---|---|
| `editions/AXP-E0002/**`, E0002 backlog rows | W1 | orchestrator | three read-only planning reviewers | orchestrator |
| operator contract/schema/evaluator plus W2 edition decision/status records | W2 | orchestrator | security + contract reviewers | orchestrator |
| `crates/proof-kernel/**` | W3 | e0002-kernel | orchestrator | orchestrator |
| `crates/proof-operator-auth/**` | W3 | e0002-auth | security reviewer | orchestrator |
| `Cargo.toml`, `Cargo.lock` | W3 | e0002-auth | orchestrator | orchestrator |
| `evals/fixtures/operator-control/**` | W3 | e0002-fixtures | contract owner | orchestrator |
| `crates/proof-storage/**` including the next migration | W4 | e0002-storage | orchestrator | orchestrator |
| `crates/proof-agent-runtime/**` | W5 | e0002-runtime | orchestrator | orchestrator |
| `crates/proof-operator-control/**`, `Cargo.toml`, `Cargo.lock` | W6 | e0002-control | security reviewer | orchestrator |
| `crates/proof-transport-http/**` read APIs | W7 | e0002-http | security reviewer | orchestrator |
| `crates/proof-transport-http/**` mutation APIs | W8 | e0002-http | security reviewer | orchestrator |
| `apps/operator-console/**` | W9 | e0002-ui | accessibility/security reviewer | orchestrator |
| `docs/dogfood/operator-control.md` | W10 | e0002-verifier | orchestrator | orchestrator |
| root/public/release integration paths listed in E0002-10 | W11 | orchestrator | product owner at Gate C | orchestrator |

No task may write an unowned surface. Each worker may also edit only its unique
handoff named in `assignments.tsv`. Shared manifests, migrations, generated
files, contracts, and release artifacts require the explicit owner above.

W3 is the only fan-out wave and has exactly three disjoint workers. Root
manifest/lock ownership is explicitly delegated to e0002-auth for W3 and
to e0002-control for W6, then returns to the orchestrator in W11. W7 and W8
intentionally reuse the same HTTP owner in different waves. No future
assignment is dispatch authority until its dependency and owner gates are
recorded.
