use crate::{load_registry, Cli};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub fn cmd_capabilities(cli: &Cli) -> Result<()> {
    let registry = load_registry(&cli.workspace)?;
    let ops: Vec<Value> = registry.operations().iter().map(|op| {
        serde_json::json!({"operation": op.operation, "domain": op.domain, "version": op.version, "governance": format!("{:?}", op.governance).to_lowercase()})
    }).collect();
    println!(
        "{}",
        serde_json::json!({"count": ops.len(), "operations": ops})
    );
    Ok(())
}

pub fn cmd_registry_list(cli: &Cli) -> Result<()> {
    let registry = load_registry(&cli.workspace)?;
    let operations: Vec<Value> = registry
        .operations()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "operation": entry.operation,
                "version": entry.version,
                "domain": entry.domain,
                "action": entry.action,
                "governance": entry.governance,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({"count": operations.len(), "operations": operations})
    );
    Ok(())
}

pub fn cmd_registry_inspect(cli: &Cli, operation: &str) -> Result<()> {
    let registry = load_registry(&cli.workspace)?;
    let entries: Vec<&proof_kernel::RegistryEntry> = registry
        .operations()
        .iter()
        .filter(|entry| entry.operation == operation)
        .collect();
    if entries.is_empty() {
        bail!("operation not found: {operation}");
    }
    let values: Vec<Value> = entries
        .iter()
        .map(|entry| serde_json::to_value(entry).map_err(anyhow::Error::from))
        .collect::<Result<_>>()?;
    if values.len() == 1 {
        println!("{}", values[0]);
    } else {
        println!(
            "{}",
            serde_json::json!({"count": values.len(), "versions": values})
        );
    }
    Ok(())
}

pub fn cmd_verify(cli: &Cli, proof_id: &str) -> Result<()> {
    let root = &cli.workspace;
    let proof_path = root
        .join(".proof/data/proofs")
        .join(format!("{proof_id}.json"));
    let raw = std::fs::read_to_string(&proof_path)
        .with_context(|| format!("proof not found: {proof_id}"))?;
    let proof: proof_kernel::Proof = serde_json::from_str(&raw).context("invalid proof JSON")?;
    let keypair = crate::Workspace::load_keypair(root)?;
    if proof.body.actor != keypair.principal_id {
        bail!(
            "proof actor {} does not match stored keypair actor {}",
            proof.body.actor,
            keypair.principal_id
        );
    }
    proof
        .verify(&keypair.signing_key.verifying_key())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    println!(
        "{}",
        serde_json::json!({
            "proof_id": proof.body.id.to_string(),
            "operation": proof.body.operation,
            "actor_id": proof.body.actor.to_string(),
            "valid": true,
        })
    );
    Ok(())
}
