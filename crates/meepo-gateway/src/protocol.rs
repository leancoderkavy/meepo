//! Gateway WebSocket protocol — JSON messages between clients and the server

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Client → Gateway request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
    /// Optional request ID for correlating responses
    #[serde(default)]
    pub id: Option<String>,
}

/// Gateway → Client response (to a specific request)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResponse {
    /// Echoed from the request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<GatewayError>,
}

/// Error in a gateway response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayError {
    pub code: i32,
    pub message: String,
}

/// Gateway → Client event (broadcast, no request ID)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayEvent {
    pub event: String,
    pub data: Value,
}

// ── Well-known methods ──

/// Methods the client can call
pub mod methods {
    pub const MESSAGE_SEND: &str = "message.send";
    pub const SESSION_LIST: &str = "session.list";
    pub const SESSION_NEW: &str = "session.new";
    pub const SESSION_HISTORY: &str = "session.history";
    pub const STATUS_GET: &str = "status.get";
}

/// Events the server broadcasts
pub mod events {
    pub const MESSAGE_RECEIVED: &str = "message.received";
    pub const TYPING_START: &str = "typing.start";
    pub const TYPING_STOP: &str = "typing.stop";
    pub const TOOL_EXECUTING: &str = "tool.executing";
    pub const STATUS_UPDATE: &str = "status.update";
    pub const SESSION_CREATED: &str = "session.created";
    pub const CANVAS_PUSH: &str = "canvas.push";
    pub const CANVAS_RESET: &str = "canvas.reset";
    pub const CANVAS_EVAL: &str = "canvas.eval";
    pub const CANVAS_SNAPSHOT: &str = "canvas.snapshot";
}

// ── Error codes ──

pub const ERR_INVALID_METHOD: i32 = -32601;
pub const ERR_INVALID_PARAMS: i32 = -32602;
pub const ERR_INTERNAL: i32 = -32603;
pub const ERR_UNAUTHORIZED: i32 = -32000;

impl GatewayResponse {
    pub fn ok(id: Option<String>, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<String>, code: i32, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(GatewayError {
                code,
                message: message.into(),
            }),
        }
    }
}

impl GatewayEvent {
    pub fn new(event: impl Into<String>, data: Value) -> Self {
        Self {
            event: event.into(),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialize() {
        let json = r#"{"method":"message.send","params":{"content":"hello","session_id":"main"}}"#;
        let req: GatewayRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "message.send");
        assert_eq!(req.params["content"], "hello");
        assert!(req.id.is_none());
    }

    #[test]
    fn test_request_with_id() {
        let json = r#"{"method":"status.get","params":{},"id":"req_1"}"#;
        let req: GatewayRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.id.as_deref(), Some("req_1"));
    }

    #[test]
    fn test_response_ok() {
        let resp = GatewayResponse::ok(Some("req_1".to_string()), serde_json::json!({"status": "ok"}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_response_err() {
        let resp = GatewayResponse::err(Some("req_2".to_string()), ERR_INVALID_METHOD, "unknown method");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32601"));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn test_event_serialize() {
        let evt = GatewayEvent::new("message.received", serde_json::json!({"content": "hi"}));
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains("\"event\":\"message.received\""));
        assert!(json.contains("\"content\":\"hi\""));
    }
}
