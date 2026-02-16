//! Gateway authentication — bearer token validation

use tracing::warn;

/// Validate a bearer token against the configured gateway token.
///
/// Returns `true` if:
/// - No token is configured (auth disabled)
/// - The provided token matches the configured token
pub fn validate_token(configured_token: &str, provided_token: &str) -> bool {
    if configured_token.is_empty() {
        return true;
    }
    if provided_token.is_empty() {
        warn!("Gateway auth: no token provided");
        return false;
    }
    // Constant-time comparison to prevent timing attacks
    constant_time_eq(configured_token.as_bytes(), provided_token.as_bytes())
}

/// Extract bearer token from an Authorization header value.
///
/// Expects format: `Bearer <token>`
pub fn extract_bearer_token(header_value: &str) -> Option<&str> {
    let trimmed = header_value.trim();
    if let Some(token) = trimmed.strip_prefix("Bearer ") {
        let token = token.trim();
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    } else {
        None
    }
}

/// Constant-time byte comparison (prevents timing side-channels)
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_token_no_config() {
        assert!(validate_token("", "anything"));
        assert!(validate_token("", ""));
    }

    #[test]
    fn test_validate_token_match() {
        assert!(validate_token("secret123", "secret123"));
    }

    #[test]
    fn test_validate_token_mismatch() {
        assert!(!validate_token("secret123", "wrong"));
        assert!(!validate_token("secret123", ""));
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("Bearer  spaced "), Some("spaced"));
        assert_eq!(extract_bearer_token("Bearer "), None);
        assert_eq!(extract_bearer_token("Basic abc123"), None);
        assert_eq!(extract_bearer_token(""), None);
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }
}
