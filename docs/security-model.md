# Security model

Proof's security model combines cryptographic identity, bounded delegation,
registry governance, canonical evidence, durable approval records, and
fail-closed recovery. This document describes the current implementation
boundary; product contracts remain authoritative.

## Security goals

Proof is designed to provide these properties:

- every governed effect is attributable to a durable principal;
- delegated authority is bounded by action, resource, time, and revocation
  state;
- inputs and outputs are bound to signed, independently verifiable evidence;
- human-only execution requires a signed request and separate human decision;
- handlers using durable exact replay do not silently duplicate completed
  mutations;
- stale workers and uncertain recovery paths fail closed; and
- public evidence excludes private keys and raw custody tokens.

## Trust boundaries

| Boundary | Trust established by | Important limitation |
|---|---|---|
| Local CLI | Filesystem access to the selected workspace and its private key | OS account and workspace permissions remain part of the trust base |
| MCP server | Stdio process launch plus the configured workspace identity | The launching client controls process configuration |
| Execution engine | Registry governance, optional delegation, handler registration, and evidence store | The embedding application must authenticate its caller |
| Human approval | Enrolled human key, signed request, signed decision, exact digest and expiry | A client-side approve button is not authority |
| Agent recovery | Durable run state, checkpoints, receipts, proofs, leases, and fences | An ambiguous in-flight mutation without a durable result is not replayed |
| Generic HTTP | Development process and network perimeter | No independent operator authentication; binds all interfaces by default |

## Identity and key custody

`proof init` creates `.proof/keypair.json`, which contains the workspace private
signing key. Human approver keys live under `.proof/approvers/`. On supported
Unix systems, Proof creates and hardens these paths with owner-only access.

Operational requirements:

- never commit `.proof/`;
- do not copy a workspace private key into fixtures, logs, or evidence;
- use `proof keypair export` for public identity data;
- use the rotation command rather than editing identity files manually;
- treat identity mismatch as a recovery event, not a prompt to overwrite one
  file with another; and
- use fresh identities and a disposable workspace for acceptance evidence.

## Delegation and governance

Delegation and registry governance are cumulative checks. A valid delegation
does not override a human-only registry entry, and an agent-executable registry
entry does not broaden a delegation.

Delegation chains are checked for root connectivity, recipient continuity,
actor binding, validity time, revocation, and authority narrowing. Execution
must recheck the current boundary before an effect, because proposal-time
authority can expire or be revoked.

## Human decisions

An approval request binds the agent, operation, version, canonical input digest,
and validity window. The enrolled human signs an approve or deny decision over
that exact request.

- Approval is a decision, not automatic resume.
- Denial grants no execution authority.
- Altered arguments require a different request.
- Expired or revoked authority fails before dispatch.
- Exact completed execution may replay its persisted result and proof without
  dispatching again.

AXP-E0006's standalone browser approval console did not receive Gate C. The
`proof approval ui` command is therefore not a supported security path. Use the
terminal signing commands for current workflows.

## Transport posture

### CLI

The CLI is appropriate for local workspaces whose filesystem and process owner
are trusted. Select the workspace explicitly in automation and protect its
identity files.

### MCP

The MCP transport uses stdio, avoiding a listening network socket. Configure an
absolute workspace path and review the client process that launches it. MCP
annotations aid clients but do not replace server-side governance.

### Generic HTTP

The current HTTP binary is a development transport. It binds `0.0.0.0:3000`,
has generic read surfaces, generates a process-local signing identity at each
start, and lacks the independent Human/workspace/server-instance session
boundary specified by AXP-E0002. Keep it on a trusted machine behind an
explicit local network boundary. It must not be presented as a production
operator control plane.

### Operator control plane

AXP-E0002 tracks a loopback-only, auth-first operator surface with volatile
sessions, protected routes, one authoritative store, and explicit control-plane
restart semantics. Lower-layer work being accepted does not make that assembled
surface released. Check the edition's [current status](../editions/AXP-E0002/status.md)
and Gate C decision before use.

## External providers and effects

Native agents can call configured model providers. Provider credentials and
model selection are deployment inputs; Proof does not make their cost or data
handling disappear.

- Use explicit allowlists and conservative budgets.
- Review provider retention and regional policies.
- Keep API keys in the process environment or a suitable secret manager.
- Use deterministic fake providers and tools for tests and acceptance evidence.
- Treat every real provider or external tool call as an external effect.

## Deployment checklist

Before relying on Proof outside local development:

- [ ] Confirm the surface has a released contract and Gate C decision.
- [ ] Use a fresh workspace with protected identity files.
- [ ] Install only reviewed registry entries and schemas.
- [ ] Register handlers for the exact operations you intend to expose.
- [ ] Bound delegation scopes and validity windows.
- [ ] Configure agent tool allowlists and all available budgets.
- [ ] Exercise approval, denial, expiry, retry, restart, and revocation paths.
- [ ] Verify signed proofs independently.
- [ ] Keep generic HTTP routes off untrusted networks.
- [ ] Preserve append-only evidence and redact secrets from diagnostics.

See the [kernel API contract](../contracts/kernel-api.md) and applicable product
contract for normative requirements.
