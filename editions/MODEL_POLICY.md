# AXP Agent Model Policy

Model choice follows task shape, risk, and measured evaluation—not prestige. Every task records its model, budget, and evaluation result. Escalate when the task fails its declared evaluation, reaches its retry limit, or exposes material risk; preserve the failed evidence and handoff.

| Model | Default assignment | Guardrail |
|---|---|---|
| `gpt-5.6-luna` | Bounded, high-volume tasks: read-only discovery, mechanical edits, fixtures, documentation, and isolated tests. | Strict path scope; no contract, security, migration, or release authority. |
| `gpt-5.6-terra` | Balanced implementation in an owned crate or module. | Must use the task's acceptance checks and stop at cross-scope needs. |
| `gpt-5.6-sol` | Contracts, security, integration, migrations, evaluation design, and material-risk review. | Named owner; independent verification where practical. |

The orchestrator may escalate `gpt-5.6-luna` → `gpt-5.6-terra` →
`gpt-5.6-sol` based on evaluation evidence, not intuition. Escalate only the
failed task, preserve its handoff, and keep the rest of the edition on the
lowest tier that passes. Model identifiers and availability are runtime
configuration; verify them before dispatch. See the
[official model guidance](https://developers.openai.com/api/docs/guides/latest-model).
