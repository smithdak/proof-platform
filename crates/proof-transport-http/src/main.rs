use proof_transport_http::{router, AppState};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState {
        version: "0.1.0".to_string(),
        started_at: chrono::Utc::now().to_rfc3339(),
    });
    let app = router().with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Proof HTTP transport listening on 0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
