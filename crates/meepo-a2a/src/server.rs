//! A2A server — receives tasks from peer agents via HTTP
//!
//! Exposes endpoints:
//! - GET  /.well-known/agent.json  — Agent card
//! - POST /a2a/tasks               — Submit a task
//! - GET  /a2a/tasks/:id           — Poll task status
//! - DELETE /a2a/tasks/:id         — Cancel a task

use std::collections::HashMap;
use std::sync::Arc;
use std::num::NonZeroUsize;
use anyhow::Result;
use chrono::Utc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;
use lru::LruCache;

use meepo_core::agent::Agent;
use meepo_core::tools::ToolRegistry;
use meepo_core::types::{ChannelType, IncomingMessage};

use crate::protocol::*;

/// Maximum request body size (1MB) to prevent OOM DoS
const MAX_REQUEST_BODY_SIZE: usize = 1_048_576;

/// Maximum number of tasks to keep in memory (LRU eviction for completed tasks)
const MAX_TASK_HISTORY: usize = 1000;

/// A2A server state
pub struct A2aServer {
    agent: Arc<Agent>,
    card: AgentCard,
    auth_token: Option<String>,
    tasks: Arc<Mutex<LruCache<String, TaskResponse>>>,
}

impl A2aServer {
    pub fn new(
        agent: Arc<Agent>,
        _registry: Arc<ToolRegistry>,
        card: AgentCard,
        auth_token: Option<String>,
        _allowed_tools: Vec<String>,
    ) -> Self {
        Self {
            agent,
            card,
            auth_token,
            tasks: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(MAX_TASK_HISTORY).unwrap(),
            ))),
        }
    }

    /// Run the A2A HTTP server
    pub async fn serve(self: Arc<Self>, port: u16) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        info!("A2A server listening on port {}", port);

        loop {
            let (stream, _addr) = listener.accept().await?;
            let server = self.clone();

            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut buf_reader = BufReader::new(reader);
                let mut request_line = String::new();

                if buf_reader.read_line(&mut request_line).await.is_err() {
                    return;
                }

                // Read headers
                let mut headers = HashMap::new();
                let mut content_length: usize = 0;
                loop {
                    let mut line = String::new();
                    if buf_reader.read_line(&mut line).await.is_err() {
                        return;
                    }
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':') {
                        let key = key.trim().to_lowercase();
                        let value = value.trim().to_string();
                        if key == "content-length" {
                            content_length = value.parse().unwrap_or(0);
                        }
                        headers.insert(key, value);
                    }
                }

                // Enforce max request body size to prevent OOM (check BEFORE allocation)
                if content_length > MAX_REQUEST_BODY_SIZE {
                    warn!("A2A request body too large: {} bytes (max {})", content_length, MAX_REQUEST_BODY_SIZE);
                    let resp = "HTTP/1.1 413 Payload Too Large\r\nContent-Type: application/json\r\n\r\n{\"error\":\"request body too large\"}";
                    let _ = writer.write_all(resp.as_bytes()).await;
                    return;
                }

                // Read body
                let mut body = vec![0u8; content_length];
                if content_length > 0 {
                    use tokio::io::AsyncReadExt;
                    if buf_reader.read_exact(&mut body).await.is_err() {
                        return;
                    }
                }

                // Check auth (constant-time comparison to prevent timing attacks)
                if let Some(ref expected_token) = server.auth_token {
                    let auth = headers.get("authorization").cloned().unwrap_or_default();
                    let is_valid = if auth.starts_with("Bearer ") {
                        let provided = auth[7..].as_bytes();
                        let expected = expected_token.as_bytes();
                        // Constant-time comparison: always compare all bytes
                        provided.len() == expected.len()
                            && provided.iter().zip(expected.iter())
                                .fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
                    } else {
                        false
                    };
                    if !is_valid {
                        let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\r\n{\"error\":\"unauthorized\"}";
                        let _ = writer.write_all(resp.as_bytes()).await;
                        return;
                    }
                }

                // Route
                let parts: Vec<&str> = request_line.split_whitespace().collect();
                if parts.len() < 2 {
                    return;
                }
                let method = parts[0];
                let path = parts[1];

                let (status, response_body) = match (method, path) {
                    ("GET", "/.well-known/agent.json") => {
                        let json = serde_json::to_string(&server.card).unwrap();
                        ("200 OK", json)
                    }
                    ("POST", "/a2a/tasks") => {
                        server.handle_submit_task(&body).await
                    }
                    ("GET", p) if p.starts_with("/a2a/tasks/") => {
                        let task_id = &p["/a2a/tasks/".len()..];
                        server.handle_get_task(task_id).await
                    }
                    ("DELETE", p) if p.starts_with("/a2a/tasks/") => {
                        let task_id = &p["/a2a/tasks/".len()..];
                        server.handle_cancel_task(task_id).await
                    }
                    _ => {
                        ("404 Not Found", r#"{"error":"not found"}"#.to_string())
                    }
                };

                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    status,
                    response_body.len(),
                    response_body
                );
                let _ = writer.write_all(response.as_bytes()).await;
            });
        }
    }

    async fn handle_submit_task(&self, body: &[u8]) -> (&'static str, String) {
        let request: TaskRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                return ("400 Bad Request", format!(r#"{{"error":"invalid request: {}"}}"#, e));
            }
        };

        let task_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let response = TaskResponse {
            task_id: task_id.clone(),
            status: TaskStatus::Submitted,
            result: None,
            created_at: now,
            completed_at: None,
        };

        {
            let mut tasks = self.tasks.lock().await;

            // Rate limit: max 100 concurrent (non-completed) tasks
            let active_count = tasks.iter()
                .filter(|(_, t)| t.status == TaskStatus::Submitted || t.status == TaskStatus::Working)
                .count();
            if active_count >= 100 {
                return ("429 Too Many Requests", r#"{"error":"too many concurrent tasks"}"#.to_string());
            }

            tasks.put(task_id.clone(), response.clone());
        }

        // Spawn background task execution
        let tasks = self.tasks.clone();
        let agent = self.agent.clone();
        let prompt = request.prompt;

        tokio::spawn(async move {
            // Mark as working
            {
                let mut t = tasks.lock().await;
                if let Some(task) = t.get_mut(&task_id) {
                    task.status = TaskStatus::Working;
                }
            }

            // Execute via agent
            let incoming = IncomingMessage {
                id: task_id.clone(),
                sender: "a2a".to_string(),
                content: prompt,
                channel: ChannelType::Internal,
                timestamp: Utc::now(),
            };
            let result = agent.handle_message(incoming).await;

            // Update status
            let mut t = tasks.lock().await;
            if let Some(task) = t.get_mut(&task_id) {
                match result {
                    Ok(outgoing) => {
                        task.status = TaskStatus::Completed;
                        task.result = Some(outgoing.content);
                        task.completed_at = Some(Utc::now());
                    }
                    Err(e) => {
                        task.status = TaskStatus::Failed;
                        task.result = Some(format!("Error: {}", e));
                        task.completed_at = Some(Utc::now());
                    }
                }
            }
        });

        let json = serde_json::to_string(&response).unwrap();
        ("201 Created", json)
    }

    async fn handle_get_task(&self, task_id: &str) -> (&'static str, String) {
        let mut tasks = self.tasks.lock().await;
        match tasks.get(task_id) {
            Some(task) => {
                let json = serde_json::to_string(task).unwrap();
                ("200 OK", json)
            }
            None => {
                ("404 Not Found", r#"{"error":"task not found"}"#.to_string())
            }
        }
    }

    async fn handle_cancel_task(&self, task_id: &str) -> (&'static str, String) {
        let mut tasks = self.tasks.lock().await;
        match tasks.get_mut(task_id) {
            Some(task) => {
                if task.status == TaskStatus::Submitted || task.status == TaskStatus::Working {
                    task.status = TaskStatus::Cancelled;
                    task.completed_at = Some(Utc::now());
                    ("200 OK", r#"{"status":"cancelled"}"#.to_string())
                } else {
                    ("409 Conflict", format!(r#"{{"error":"task already {}"}}"#, task.status))
                }
            }
            None => {
                ("404 Not Found", r#"{"error":"task not found"}"#.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_card_json() {
        let card = AgentCard {
            name: "meepo".to_string(),
            description: "Personal AI agent".to_string(),
            url: "http://localhost:8081".to_string(),
            capabilities: vec!["file_operations".to_string()],
            authentication: AuthConfig {
                schemes: vec!["bearer".to_string()],
            },
        };
        let json = serde_json::to_string_pretty(&card).unwrap();
        assert!(json.contains("meepo"));
        assert!(json.contains("bearer"));
    }

    #[test]
    fn test_task_status_transitions() {
        let mut response = TaskResponse {
            task_id: "test-1".to_string(),
            status: TaskStatus::Submitted,
            result: None,
            created_at: Utc::now(),
            completed_at: None,
        };

        assert_eq!(response.status, TaskStatus::Submitted);
        response.status = TaskStatus::Working;
        assert_eq!(response.status, TaskStatus::Working);
        response.status = TaskStatus::Completed;
        response.result = Some("Done".to_string());
        assert_eq!(response.status, TaskStatus::Completed);
    }
}
