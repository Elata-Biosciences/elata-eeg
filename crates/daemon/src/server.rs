use crate::api::{self, AppState};
use axum::{
    body::Body,
    extract::{State, WebSocketUpgrade},
    response::{IntoResponse, Response},
    routing::get,
};
use http::StatusCode;
use log::error;
use std::{any::Any, net::SocketAddr};
use tower_http::cors::{Any as CorsAny, CorsLayer};

fn handle_panic(err: Box<dyn Any + Send + 'static>) -> Response<Body> {
    let details = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "Unknown panic message".to_string()
    };

    error!("PANIC CAUGHT: {}", details);

    // Optionally, you can also try to get a backtrace if RUST_BACKTRACE is set
    // This requires the backtrace crate and might be more involved.
    // For now, just logging the message is a huge step forward.

    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from(format!("Internal Server Error: {}", details)))
        .unwrap()
}

#[axum::debug_handler]
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| async move { state.broker.add_client(socket) })
}

pub async fn run(
    state: AppState,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let app = api::create_router()
        .route("/ws/data", get(websocket_handler))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(CorsAny)
                .allow_methods(CorsAny)
                .allow_headers(CorsAny),
        )
;

    // run it
    let addr = SocketAddr::from(([0, 0, 0, 0], 9000));
    tracing::info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
        })
        .await?;

    Ok(())
}