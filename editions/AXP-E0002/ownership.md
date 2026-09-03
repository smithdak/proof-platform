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
| historical derived operator-control fixture closure | W5 | e0002-fixtures | blocked review set preserved | orchestrator |
| exact reads/manifest/evaluator/v3-packet semantic repair and bounded edition records | W6 | orchestrator | three fresh read-only reviewers after W7 | orchestrator |
| exact 158-file derived operator-control fixture closure | W7 | e0002-fixtures | three fresh read-only reviewers after quiescence | orchestrator |
| `crates/proof-kernel/**` four-profile source alignment | W8 | e0002-kernel | fresh independent reviewer | orchestrator |
| `crates/proof-storage/**` including migration 14 | W9 | e0002-storage | orchestrator | orchestrator |
| `crates/proof-agent-runtime/**` | W9 | e0002-runtime | orchestrator | orchestrator |
| `crates/proof-operator-control/**`, `Cargo.toml`, `Cargo.lock` | W9 | e0002-control | security reviewer | orchestrator |
| `conformance/**`, `Cargo.lock` backend integration | W10 | e0002-backend-verifier | orchestrator | orchestrator |
| `crates/proof-transport-http/**` read APIs | W10 | e0002-http | security reviewer | orchestrator |
| `crates/proof-transport-http/**` mutation APIs | W11 | e0002-http | security reviewer | orchestrator |
| `apps/operator-console/**` | W12 | e0002-ui | accessibility/security reviewer | orchestrator |
| `crates/proof-operator-control/**`, `Cargo.lock`, generated `apps/operator-console/dist/**` | W13 | e0002-assembly | security reviewer | orchestrator |
| `docs/dogfood/operator-control.md` | W14 | e0002-verifier | orchestrator | orchestrator |
| root/public/release integration paths listed in E0002-10 | W15 | orchestrator | product owner at Gate C | orchestrator |

No task may write an unowned surface. Each worker may also edit only its unique
handoff named in `assignments.tsv`. Shared manifests, migrations, generated
files, contracts, and release artifacts require the explicit owner above.

W4 through W8 are serialized single-writer repair waves; W5 is blocked
historical evidence, while W8 and shifted W9 are done. W3 and shifted W9 each
had exactly three disjoint workers; W10 has two dependency-ready but
undispatched lanes. Root manifest/lock ownership
is explicitly delegated to e0002-auth for W3 and e0002-control for W9. In W10,
both package-manifest owners freeze their deltas
before e0002-backend-verifier alone reconciles `Cargo.lock`; neither runs Cargo
build/test commands until the resulting lock is stable. The lock owner may run
only the Gate-B-frozen serialized lock-reconciliation Cargo command during that
barrier. W13 then gives e0002-assembly exclusive
`Cargo.lock` ownership for the later control-crate dependency delta. Root
manifest/lock ownership returns to the orchestrator in W15. Each manifest
owner must stabilize its delta before other same-wave Cargo build/test commands
begin, and writers must quiesce before reverse-impact acceptance. W7 and W8
from the prior graph are now W10 and W11 and reuse the same HTTP owner in
different waves. W10 package-manifest deltas
likewise stabilize before concurrent Cargo build/test commands, and both
writers quiesce before impact acceptance. W13 may reproduce only the W12-frozen
generated UI bundle under `apps/operator-console/dist/**`; UI source remains
read-only. No future assignment is dispatch authority until its dependency and
owner gates are recorded.
