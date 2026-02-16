//! Embedded WebChat UI — serves the built React SPA from the binary

use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "ui/dist/"]
struct WebChatAssets;

/// Serve the WebChat SPA index.html
pub async fn index_handler() -> impl IntoResponse {
    match WebChatAssets::get("index.html") {
        Some(content) => Html(content.data.to_vec()).into_response(),
        None => (StatusCode::NOT_FOUND, "WebChat UI not built. Run: cd crates/meepo-gateway/ui && npm run build").into_response(),
    }
}

/// Serve static assets (JS, CSS, etc.)
pub async fn static_handler(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    match WebChatAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .body(axum::body::Body::from(content.data.to_vec()))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(axum::body::Body::empty())
                        .expect("fallback response")
                })
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::from("Not found"))
            .expect("404 response"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webchat_assets_embedded() {
        // The dist/ folder should be embedded at compile time
        let index = WebChatAssets::get("index.html");
        // This test passes if the UI was built before cargo test
        if let Some(content) = index {
            assert!(!content.data.is_empty());
            let html = String::from_utf8_lossy(&content.data);
            assert!(html.contains("<!DOCTYPE html>") || html.contains("<html"));
        }
    }

    #[test]
    fn test_webchat_assets_list() {
        let files: Vec<_> = WebChatAssets::iter().collect();
        // Should have at least index.html if built
        if !files.is_empty() {
            assert!(files.iter().any(|f| f.as_ref() == "index.html"));
        }
    }
}
