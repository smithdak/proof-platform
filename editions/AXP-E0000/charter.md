# AXP-E0000 Charter — One Product Truth

Status: approved
Owner: product owner
Orchestrator: Codex primary agent
Base revision: `b1ff2ac8e49eddc26b4977268bb9184d9a0f7385`
Gate A approval: product owner, 2026-08-29
Gate B security approval: D-E0000-002, product owner, 2026-08-29

## Outcome

By the end of E0000, Proof has a tracked, repeatable swarm process and one
truthful walking-skeleton product story: content operations are contract-
aligned, governed, evidence-producing, and executable through deterministic
conformance and Release Manager automation.

## Scope

- Track edition artifacts and make swarm ownership/handoffs explicit.
- Remediate the tracked `.proof` key/config/database exposure as an owner-
  approved P0. Do not remove the files as part of this edition.
- Automate reverse-dependency test impact selection.
- Reconcile architecture, kernel API, domain definitions, registry, and code.
- Complete governed `edition.create` and `changeset.commit`.
- Add executable conformance for the eight canonical content operations.
- Preserve and harden deterministic Release Manager evidence.

## Non-goals

No new domain, UI redesign, live-provider requirement, PostgreSQL rollout,
parent/child runtime, or broad security-history rewrite is included. Those are
later editions unless an owner decision changes this charter.

## Success measures and gates

The owner accepts only when every E0000 backlog item has linked evidence,
scoped tests are green for changed crates and dependents, conformance passes,
the deterministic release trace evaluates 10/10, and all P0/high risks have a
written disposition. Security remediation requires an explicit owner decision
and records what was rotated, quarantined, or deferred; files remain present.

## Wave shape

One orchestrator plus at most three concurrent workers. Suggested waves are:
records; process/security; contract freeze; shared replay design/implementation;
content/registry; three peer transports;
conformance; deterministic automation; integration/owner review.
Root files, shared contracts, migrations, and final evidence always have one
named writer.

## Delivery budget

- One orchestrator and at most three concurrent workers.
- Lowest eligible model tier per `editions/MODEL_POLICY.md`.
- One primary attempt and one bounded repair attempt per task before escalation.
- No live-provider requirement, paid external effect, or expanded edition scope
  without Gate B approval.
