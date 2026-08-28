use proof_transport_http::{router, AppState};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let workspace = std::env::var("PROOF_WORKSPACE").unwrap_or_else(|_| ".".to_string());
    let state = Arc::new(AppState {
        workspace_path: workspace,
        version: "0.1.0".to_string(),
    });
    let app = router(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Proof HTTP transport listening on 0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
