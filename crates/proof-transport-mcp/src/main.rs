use std::path::PathBuf;
use std::sync::Arc;

use proof_transport_mcp::{load_workspace_keypair, load_workspace_registry, McpServer};

fn main() {
    if let Err(error) = run() {
        eprintln!("proof-mcp: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(workspace_path) = workspace_argument()? else {
        return Ok(());
    };
    let identity = load_workspace_keypair(&workspace_path)?;
    let registry = load_workspace_registry(&workspace_path)?;
    let storage_path = workspace_path.join(".proof/storage/storage.db");
    if let Some(storage_directory) = storage_path.parent() {
        std::fs::create_dir_all(storage_directory)?;
    }
    let workspace_store = Arc::new(proof_storage::SqliteStore::open(&storage_path)?);
    let mut server =
        McpServer::new_with_storage(registry, identity, workspace_path, workspace_store);
    for handler in proof_content::handlers::content_handlers()
        .into_iter()
        .chain(proof_commerce::handlers::commerce_handlers())
        .chain(proof_workflow::handlers::workflow_handlers())
        .chain(proof_analytics::handlers::analytics_handlers())
    {
        server.register_handler(handler);
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    server.serve_stdio(stdin.lock(), stdout.lock())?;
    Ok(())
}

fn workspace_argument() -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let mut workspace_path = std::env::current_dir()?;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--workspace" | "-w" => {
                let value = arguments
                    .next()
                    .ok_or("--workspace requires a path argument")?;
                workspace_path = PathBuf::from(value);
            }
            "--help" | "-h" => {
                println!("Usage: proof-mcp [--workspace <PATH>]");
                return Ok(None);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(Some(workspace_path))
}
