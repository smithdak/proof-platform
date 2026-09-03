# Core concepts

Proof separates the question “can this actor perform this operation?” from
“what code implements it?” and “how can another party verify what happened?”
That separation is the foundation of the platform.

## The execution model

A governed operation follows one path:

```text
Registry entry
    → execution context
    → governance and delegation checks
    → registered handler
    → canonical output
    → signed proof
    → durable evidence
```

Transports adapt protocols to this path. They do not become independent
sources of authority.

## Operations and the registry

An operation is a versioned capability such as `schema.create::v1` or
`order.fulfill::v1`. Its registry entry describes:

- the stable operation name and version;
- its domain and authority action;
- input and output schemas;
- governance and consequence classification;
- idempotency and evidence contracts; and
- an optional benchmark contract.

The registry is data. Adding an entry makes the capability discoverable, but
execution still requires a handler registered for the exact logical operation.
A discoverable entry without a handler fails closed.

## Principals and authority

A principal is a durable identity representing a human, agent, or service.
Proof uses Ed25519 signatures to bind actions and evidence to principals.

A delegation grants a recipient a bounded subset of an issuer's authority. It
constrains actions, resource scopes, validity time, and revocation state.
Delegation chains must connect the trusted root to the executing actor, and
each child grant must preserve or narrow its parent's authority.

Registry governance still applies when no delegation chain is supplied. The
embedding transport or application is responsible for establishing any
additional caller boundary.

## Human-only operations

Human-only governance is a cryptographic workflow, not a confirmation dialog:

1. An agent requests an exact operation with canonical arguments.
2. Proof persists and signs an approval request bound to that actor, operation,
   version, input digest, and expiry.
3. An enrolled human signs an approve or deny decision.
4. The runtime verifies both records and resumes the same run and step.
5. Approval permits execution; it does not execute automatically.

Unsigned client acceptance is never authority. A denial grants no execution
right. A completed approved request replays its persisted result instead of
dispatching the mutation again.

## Proofs, audit records, and evaluations

These records answer different questions:

| Record | Question answered |
|---|---|
| Proof | Who executed which versioned operation over which input and output? |
| Execution context | Under what workspace, actor, authority, and time did execution occur? |
| Run event | What happened during the durable agent lifecycle? |
| Approval evidence | Which exact request did a human approve or deny? |
| Evaluation | Did a sealed trace satisfy a named deterministic policy? |

A proof is signed operation evidence. An audit trail is the ordered chronology
around it. An evaluation is a later assertion over a sealed trace. None is an
implicit substitute for another.

## Canonical data and digests

Structured operation data is canonicalized before hashing. Kernel evidence
uses domain-separated BLAKE3-256 `ContentDigest` values, serialized in current
proof envelopes as 64 lowercase hexadecimal characters. Independent parties
can reconstruct the signed payload from the same logical JSON.

Some content-domain snapshot identifiers retain algorithm-qualified
`sha256:<hex>` values for their versioned v1 contracts. Digest algorithms and
wire representations are therefore part of the contract; callers must not
infer one from another field.

## Durable agent runs

An agent definition binds instructions, provider and model, an explicit tool
allowlist, and hard budgets. A run records how that definition pursues one
goal.

- A **run** owns the goal, actor, state, budget use, and terminal result.
- A **step** is one exact operation attempt.
- A **checkpoint** preserves resumable runtime state before an effect boundary.
- An **event** appends a digest-addressed lifecycle fact.
- A **retry** creates a new attempt linked to the original; it does not rewrite
  history.

One-shot tool calls terminate automatically. Session runs can compose several
operations and are completed or cancelled explicitly.

## Idempotency, replay, and recovery

For handlers that select durable exact replay, idempotency prevents an
ambiguous retry from becoming a second mutation. A completed request with the
same scoped key and canonical input returns its original result and proof.
Reusing the key with different input fails.

The runtime checkpoints before model and tool boundaries. After restart it can
reuse a completed step or approval execution. If a mutation may have started
but no durable result exists, recovery fails closed rather than guessing.

## Vocabulary

| Term | Meaning |
|---|---|
| Workspace | Filesystem root containing one `.proof/` identity and data tree |
| Actor | Principal performing the operation |
| Handler | Domain code registered for one logical operation |
| Consequence | Contract classification of an operation's effect |
| Governance | Whether and how an operation may execute |
| Custody | Sensitive runtime material retained only at the boundary that owns it |
| Fence | Monotonic authority marker rejecting stale workers or writes |
| Gate C | Product-owner release decision for an AXP Edition |

For exact field shapes, use the [kernel API contract](../contracts/kernel-api.md)
and [domain definitions](../contracts/domain-definitions.md).
