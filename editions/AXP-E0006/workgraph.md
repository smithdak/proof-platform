# AXP-E0006 Workgraph

Edition: `AXP-E0006`

| Task | Wave | Owner/model | Owned paths | Depends on | Acceptance | Status |
|---|---|---|---|---|---|---|
| E0006-01 | W1 | orchestrator / `gpt-5.6-sol` | proposed session contract, evaluation, backlog, and edition records | E0001 Gate C disposition | Exact threat model, charter, protocol, limits, task packets, and Gate A/B decision packet validate | done; Gate A/B approved by D-E0006-004 |
| E0006-02 | W2 | e0006-security / `gpt-5.6-sol` | `proof-transport-cli`, exact `Cargo.lock` delta, unique handoff | E0006-01 + Gate A/B | Clean URL/output, one long-poll terminal-confirmed bootstrap, scoped session, expiry/revocation versus signing lease, fresh governance, unchanged signatures | done; 117/117 host tests and independent source review PASS |
| E0006-03 | W3 | e0006-verifier / `gpt-5.6-sol` | redacted dogfood evidence plus unique handoff | E0006-02 | Browser/process/router/security evidence passes exact 14/14 policy with no captured credential | blocked; D-E0006-013 reached terminal verification but the current attached tab failed closed before session/app evidence, and final run authority is consumed |
| E0006-04 | W4 | orchestrator / `gpt-5.6-sol` | public guidance, contract reconciliation, release records, root integration | E0006-01..03 | Scoped impact plus quiescent gate, rollback and limitations, dated Gate C decision | pending |

## Dependency flow

```text
E0001 D-E0001-020 defer; writers quiescent
                    |
                    v
E0006-01 threat model + contract + evaluation
                    |
              Gate A + Gate B
                    |
                    v
E0006-02 single-owner CLI security implementation
                    |
                    v
E0006-03 independent browser/security verification
                    |
                    v
E0006-04 quiescent integration + Gate C
                    |
                    v
E0002 may enter its own Gate A only after E0006 release
```

## Wave gates

- W1 is planning/contract work only. It may not modify CLI behavior, start the
  approval server, create a credential, read private workspace material, or
  claim owner approval. E0006-01 remains `review` until both Gate A and the
  exact security Gate B packet are approved.
- W2 has one writer for the entire CLI crate because authentication state,
  HTML, server routing, process behavior, and tests form one security boundary.
  That writer also owns the exact already-locked `rustix` dependency delta in
  `Cargo.lock`. It starts only after E0006-01 is `done` and the decisions are
  recorded.
- W3 starts only after W2 is quiescent and its scoped gates pass. The verifier
  must not have authored E0006-02 and must use a disposable synthetic workspace.
  No secret-bearing screenshot, log, browser export, or test artifact may be
  retained.
- W4 starts after every worker quiesces. The orchestrator reproduces scoped and
  reverse-impact checks, reconciles the public contract/guidance, runs the
  explicit edition final verifier, and presents Gate C without self-approval.

Cross-owner requests and dependency changes are recorded in `decisions.md`.
