//! Gateway WebSocket server — Axum-based HTTP + WS server

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

use crate::auth;
use crate::events::EventBus;
use crate::protocol::{
    self, GatewayEvent, GatewayRequest, GatewayResponse, ERR_INVALID_METHOD,
    ERR_INVALID_PARAMS,
};
use crate::session::SessionManager;

/// Shared state for all WebSocket connections
#[derive(Clone)]
pub struct GatewayState {
    pub sessions: Arc<SessionManager>,
    pub events: EventBus,
    pub auth_token: String,
    pub start_time: std::time::Instant,
}

/// The gateway server
pub struct GatewayServer {
    state: GatewayState,
    bind: SocketAddr,
}

impl GatewayServer {
    /// Create a new gateway server
    pub fn new(bind: SocketAddr, auth_token: String) -> Self {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(256),
            auth_token,
            start_time: std::time::Instant::now(),
        };
        Self { state, bind }
    }

    /// Get a reference to the event bus (for broadcasting from outside)
    pub fn event_bus(&self) -> &EventBus {
        &self.state.events
    }

    /// Get a reference to the session manager
    pub fn sessions(&self) -> &Arc<SessionManager> {
        &self.state.sessions
    }

    /// Build the Axum router
    pub fn router(&self) -> Router {
        Router::new()
            .route("/ws", get(ws_handler))
            .route("/api/status", get(status_handler))
            .route("/api/sessions", get(sessions_handler))
            .route("/", get(crate::webchat::index_handler))
            .route("/assets/{*path}", get(crate::webchat::static_handler))
            .layer(CorsLayer::permissive())
            .with_state(self.state.clone())
    }

    /// Start the server (blocks until shutdown)
    pub async fn run(self) -> anyhow::Result<()> {
        let router = self.router();
        let listener = tokio::net::TcpListener::bind(self.bind).await?;
        info!("Gateway listening on {}", self.bind);

        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;

        Ok(())
    }

    /// Start the server in the background, returning a handle
    pub fn spawn(self) -> tokio::task::JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move { self.run().await })
    }
}

// ── HTTP Handlers ──

async fn status_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let sessions = state.sessions.count().await;
    let uptime = state.start_time.elapsed().as_secs();
    let clients = state.events.subscriber_count();

    axum::Json(serde_json::json!({
        "status": "ok",
        "sessions": sessions,
        "connected_clients": clients,
        "uptime_secs": uptime,
    }))
}

async fn sessions_handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    // Auth check for REST endpoints
    if !check_auth(&state.auth_token, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let sessions = state.sessions.list().await;
    Ok(axum::Json(serde_json::json!({ "sessions": sessions })))
}

// ── WebSocket Handler ──

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<GatewayState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    // Auth check on upgrade
    if !check_auth(&state.auth_token, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    info!("WebSocket connection from {}", addr);
    ws.on_upgrade(move |socket| handle_ws(socket, state, addr))
        .into_response()
}

async fn handle_ws(socket: WebSocket, state: GatewayState, addr: SocketAddr) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut event_rx = state.events.subscribe();

    use futures_util::{SinkExt, StreamExt};

    // Spawn a task to forward broadcast events to this client
    let send_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(e) => {
                            error!("Failed to serialize event: {}", e);
                            continue;
                        }
                    };
                    if ws_sender.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Client {} lagged by {} events", addr, n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Process incoming messages from this client
    while let Some(msg) = ws_receiver.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => {
                debug!("Client {} disconnected", addr);
                break;
            }
            Ok(_) => continue,
            Err(e) => {
                warn!("WebSocket error from {}: {}", addr, e);
                break;
            }
        };

        let response = handle_request(&state, &msg).await;
        if let Err(e) = serde_json::to_string(&response) {
            error!("Failed to serialize response: {}", e);
            continue;
        }

        // We can't send directly since ws_sender moved; instead broadcast the response
        // as a targeted event. In a production system we'd use a per-client sender.
        // For now, broadcast the response (clients filter by request ID).
        state.events.broadcast(GatewayEvent::new(
            "response",
            serde_json::to_value(&response).unwrap_or_default(),
        ));
    }

    send_task.abort();
    info!("Client {} disconnected", addr);
}

async fn handle_request(state: &GatewayState, raw: &str) -> GatewayResponse {
    let req: GatewayRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => {
            return GatewayResponse::err(
                None,
                ERR_INVALID_PARAMS,
                format!("Invalid JSON: {}", e),
            );
        }
    };

    let id = req.id.clone();

    match req.method.as_str() {
        protocol::methods::STATUS_GET => {
            let sessions = state.sessions.count().await;
            let uptime = state.start_time.elapsed().as_secs();
            let clients = state.events.subscriber_count();
            GatewayResponse::ok(
                id,
                serde_json::json!({
                    "status": "ok",
                    "sessions": sessions,
                    "connected_clients": clients,
                    "uptime_secs": uptime,
                }),
            )
        }

        protocol::methods::SESSION_LIST => {
            let sessions = state.sessions.list().await;
            GatewayResponse::ok(id, serde_json::to_value(&sessions).unwrap_or_default())
        }

        protocol::methods::SESSION_NEW => {
            let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("Untitled");
            let session = state.sessions.create(name).await;

            // Broadcast session creation event
            state.events.broadcast(GatewayEvent::new(
                protocol::events::SESSION_CREATED,
                serde_json::to_value(&session).unwrap_or_default(),
            ));

            GatewayResponse::ok(id, serde_json::to_value(&session).unwrap_or_default())
        }

        protocol::methods::SESSION_HISTORY => {
            let session_id = req
                .params
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("main");

            match state.sessions.get(session_id).await {
                Some(_session) => {
                    // TODO: Load history from KnowledgeDb once integrated
                    GatewayResponse::ok(
                        id,
                        serde_json::json!({
                            "session_id": session_id,
                            "messages": [],
                        }),
                    )
                }
                None => GatewayResponse::err(
                    id,
                    ERR_INVALID_PARAMS,
                    format!("Session '{}' not found", session_id),
                ),
            }
        }

        protocol::methods::MESSAGE_SEND => {
            let content = req.params.get("content").and_then(|v| v.as_str());
            let session_id = req
                .params
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("main");

            let content = match content {
                Some(c) if !c.is_empty() => c,
                _ => {
                    return GatewayResponse::err(
                        id,
                        ERR_INVALID_PARAMS,
                        "Missing or empty 'content' parameter",
                    );
                }
            };

            // Record activity
            state.sessions.record_activity(session_id).await;

            // Broadcast typing indicator
            state.events.broadcast(GatewayEvent::new(
                protocol::events::TYPING_START,
                serde_json::json!({"session_id": session_id}),
            ));

            // TODO: Route message to Agent for processing
            // For now, echo back a placeholder
            let response_text = format!("[Gateway] Received: {}", content);

            state.events.broadcast(GatewayEvent::new(
                protocol::events::TYPING_STOP,
                serde_json::json!({"session_id": session_id}),
            ));

            // Broadcast the response as a message event
            state.events.broadcast(GatewayEvent::new(
                protocol::events::MESSAGE_RECEIVED,
                serde_json::json!({
                    "session_id": session_id,
                    "content": response_text,
                    "role": "assistant",
                }),
            ));

            GatewayResponse::ok(
                id,
                serde_json::json!({
                    "session_id": session_id,
                    "content": response_text,
                }),
            )
        }

        _ => GatewayResponse::err(
            id,
            ERR_INVALID_METHOD,
            format!("Unknown method: {}", req.method),
        ),
    }
}

fn check_auth(configured_token: &str, headers: &HeaderMap) -> bool {
    if configured_token.is_empty() {
        return true;
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(auth::extract_bearer_token);

    match token {
        Some(t) => auth::validate_token(configured_token, t),
        None => {
            // Also check query param for WebSocket connections
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_auth_no_config() {
        let headers = HeaderMap::new();
        assert!(check_auth("", &headers));
    }

    #[test]
    fn test_check_auth_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret123".parse().unwrap());
        assert!(check_auth("secret123", &headers));
    }

    #[test]
    fn test_check_auth_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert!(!check_auth("secret123", &headers));
    }

    #[test]
    fn test_check_auth_missing_header() {
        let headers = HeaderMap::new();
        assert!(!check_auth("secret123", &headers));
    }

    #[tokio::test]
    async fn test_handle_request_status() {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(16),
            auth_token: String::new(),
            start_time: std::time::Instant::now(),
        };
        let resp = handle_request(&state, r#"{"method":"status.get","params":{}}"#).await;
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_handle_request_session_list() {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(16),
            auth_token: String::new(),
            start_time: std::time::Instant::now(),
        };
        let resp = handle_request(&state, r#"{"method":"session.list","params":{}}"#).await;
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn test_handle_request_session_new() {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(16),
            auth_token: String::new(),
            start_time: std::time::Instant::now(),
        };
        let resp = handle_request(
            &state,
            r#"{"method":"session.new","params":{"name":"Research"}}"#,
        )
        .await;
        assert!(resp.result.is_some());
        assert_eq!(state.sessions.count().await, 2);
    }

    #[tokio::test]
    async fn test_handle_request_unknown_method() {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(16),
            auth_token: String::new(),
            start_time: std::time::Instant::now(),
        };
        let resp = handle_request(&state, r#"{"method":"unknown","params":{}}"#).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, ERR_INVALID_METHOD);
    }

    #[tokio::test]
    async fn test_handle_request_invalid_json() {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(16),
            auth_token: String::new(),
            start_time: std::time::Instant::now(),
        };
        let resp = handle_request(&state, "not json").await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_handle_request_message_send() {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(16),
            auth_token: String::new(),
            start_time: std::time::Instant::now(),
        };
        let resp = handle_request(
            &state,
            r#"{"method":"message.send","params":{"content":"hello","session_id":"main"}}"#,
        )
        .await;
        assert!(resp.result.is_some());
        let session = state.sessions.get("main").await.unwrap();
        assert_eq!(session.message_count, 1);
    }

    #[tokio::test]
    async fn test_handle_request_message_send_empty() {
        let state = GatewayState {
            sessions: Arc::new(SessionManager::new()),
            events: EventBus::new(16),
            auth_token: String::new(),
            start_time: std::time::Instant::now(),
        };
        let resp = handle_request(
            &state,
            r#"{"method":"message.send","params":{"content":""}}"#,
        )
        .await;
        assert!(resp.error.is_some());
    }
}
