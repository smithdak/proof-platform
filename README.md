# Proof Platform

A governed, agent-native platform where autonomous software discovers, composes, and executes operations across any domain, producing cryptographic evidence for every governed transition.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full foundational design document.

## Quickstart

```bash
# Initialize a workspace
proof init

# Create a content schema
proof schema-create --name "Article" --fields '[{"name":"title","field_type":"text","required":true}]'

# Create an object
proof object-create --schema-id <id> --locale "en-US" --data '{"title":"Hello"}'

# Create a changeset, edition, and release
proof changeset-create --intent "Initial content"
proof edition-create --changeset-id <id>
proof release-publish --edition-id <id> --environment "preview"

# Check status
proof status
proof capabilities
```

## Crates

| Crate | Purpose |
|---|---|
| `proof-kernel` | Identity, delegation, evidence, registry, canonical JSON |
| `proof-content` | Content governance domain (first domain module) |
| `proof-storage` | SQLite storage adapter |
| `proof-transport-http` | HTTP/REST API (axum) |
| `proof-transport-mcp` | MCP tool generation for agents |
| `proof-transport-cli` | CLI for developers and scripting |

## Testing

```bash
cargo test --workspace
```

## Status

- ✅ Kernel: canonical JSON, BLAKE3 digests, Ed25519 identity, delegation, evidence
- ✅ Content domain: schema, object lifecycle, changeset, edition, release
- ✅ CLI: full governance lifecycle (init → schema → object → changeset → edition → release)
- ✅ HTTP: capabilities, schemas, objects, proofs endpoints
- ✅ MCP: tool generation from registry
- ✅ Storage: SQLite with full schema
- 🔲 PostgreSQL storage adapter
- 🔲 SAM mesh integration
- 🔲 Content domain: full 45-capability wave (per ADR-0014)
- 🔲 Analytics/commerce/workflow domain modules
