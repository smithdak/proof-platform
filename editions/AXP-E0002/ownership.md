# Ownership Matrix

| Surface/path | Wave | Single writer | Reviewer | Integration owner |
|---|---|---|---|---|
| `editions/AXP-E0002/**`, E0002 backlog rows | W1 | orchestrator | three read-only planning reviewers | orchestrator |
| operator contract/schema/evaluator plus W2 edition decision/status records | W2 | orchestrator | security + contract reviewers | orchestrator |
| `crates/proof-kernel/**` | W3 | e0002-kernel | orchestrator | orchestrator |
| `crates/proof-operator-auth/**` | W3 | e0002-auth | security reviewer | orchestrator |
| `Cargo.toml`, `Cargo.lock` | W3 | e0002-auth | orchestrator | orchestrator |
| `evals/fixtures/operator-control/**` | W3 | e0002-fixtures | contract owner | orchestrator |
| exact contract/reads/manifest/evaluator repair and bounded edition records | W4 | orchestrator | three fresh read-only reviewers after W5 | orchestrator |
| exact derived operator-control fixture closure | W5 | e0002-fixtures | three fresh read-only reviewers after quiescence | orchestrator |
| `crates/proof-storage/**` including migration 14 | W6 | e0002-storage | orchestrator | orchestrator |
| `crates/proof-agent-runtime/**` | W6 | e0002-runtime | orchestrator | orchestrator |
| `crates/proof-operator-control/**`, `Cargo.toml`, `Cargo.lock` | W6 | e0002-control | security reviewer | orchestrator |
| `conformance/**`, `Cargo.lock` backend integration | W7 | e0002-backend-verifier | orchestrator | orchestrator |
| `crates/proof-transport-http/**` read APIs | W7 | e0002-http | security reviewer | orchestrator |
| `crates/proof-transport-http/**` mutation APIs | W8 | e0002-http | security reviewer | orchestrator |
| `apps/operator-console/**` | W9 | e0002-ui | accessibility/security reviewer | orchestrator |
| `crates/proof-operator-control/**`, `Cargo.lock`, generated `apps/operator-console/dist/**` | W10 | e0002-assembly | security reviewer | orchestrator |
| `docs/dogfood/operator-control.md` | W11 | e0002-verifier | orchestrator | orchestrator |
| root/public/release integration paths listed in E0002-10 | W12 | orchestrator | product owner at Gate C | orchestrator |

No task may write an unowned surface. Each worker may also edit only its unique
handoff named in `assignments.tsv`. Shared manifests, migrations, generated
files, contracts, and release artifacts require the explicit owner above.

W4 and W5 are serialized single-writer planning waves. W3 and shifted W6 each
have exactly three disjoint workers; W7 has two. Root manifest/lock ownership
is explicitly delegated to e0002-auth for W3 and e0002-control for W6. In W7,
both package-manifest owners freeze their deltas
before e0002-backend-verifier alone reconciles `Cargo.lock`; neither runs Cargo
build/test commands until the resulting lock is stable. The lock owner may run
only the Gate-B-frozen serialized lock-reconciliation Cargo command during that
barrier. W10 then gives e0002-assembly exclusive
`Cargo.lock` ownership for the later control-crate dependency delta. Root
manifest/lock ownership returns to the orchestrator in W12. Each manifest
owner must stabilize its delta before other same-wave Cargo build/test commands
begin, and writers must quiesce before reverse-impact acceptance. W7 and W8
reuse the same HTTP owner in different waves. W7 package-manifest deltas
likewise stabilize before concurrent Cargo build/test commands, and both
writers quiesce before impact acceptance. W10 may reproduce only the W9-frozen
generated UI bundle under `apps/operator-console/dist/**`; UI source remains
read-only. No future assignment is dispatch authority until its dependency and
owner gates are recorded.
