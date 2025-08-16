use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

// A mock WebSocketBroker for testing purposes.
#[derive(Default)]
pub struct WebSocketBroker;

// The application state.
#[derive(Clone)]
struct AppState {
    broker: Arc<WebSocketBroker>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app_state = AppState {
        broker: Arc::new(WebSocketBroker::default()),
    };

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .with_state(app_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 9999));
    let listener = TcpListener::bind(&addr).await.unwrap();
    info!("listening on {}", addr);
    axum::serve(listener, app.into_make_service()).await.unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(_state): State<AppState>,
) -> Response {
    info!("WebSocket upgrade request received");
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(_socket: WebSocket) {
    info!("Connection successful");
}