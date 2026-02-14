//! System interaction tools

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;
use tracing::{debug, warn};

use super::{ToolHandler, json_schema};

/// Validate file path to prevent path traversal attacks
/// Returns the validated PathBuf or an error if the path is unsafe
fn validate_file_path(path: &str, for_write: bool) -> Result<PathBuf> {
    // Check for suspicious patterns before canonicalization
    if path.contains("..") {
        return Err(anyhow::anyhow!(
            "Path contains '..' which is not allowed for security reasons"
        ));
    }

    let path_buf = PathBuf::from(path);

    // For reads, the file must exist so we can canonicalize
    // For writes, we validate the parent directory
    let canonical_path = if for_write {
        // For writes, check if parent exists and canonicalize parent
        if let Some(parent) = path_buf.parent() {
            if parent.as_os_str().is_empty() {
                // No parent means current directory
                std::env::current_dir()
                    .context("Failed to get current directory")?
                    .join(path_buf.file_name().unwrap())
            } else if parent.exists() {
                // Parent exists, canonicalize it and append filename
                let canonical_parent = parent
                    .canonicalize()
                    .context("Failed to canonicalize parent directory")?;
                canonical_parent.join(path_buf.file_name().unwrap())
            } else {
                // Parent doesn't exist, just convert to absolute path
                if path_buf.is_absolute() {
                    path_buf
                } else {
                    std::env::current_dir()
                        .context("Failed to get current directory")?
                        .join(path_buf)
                }
            }
        } else {
            // No parent directory (shouldn't happen)
            path_buf
        }
    } else {
        // For reads, file must exist
        path_buf
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize path: {}", path))?
    };

    // Check if the resolved path is within the user's home directory, current directory, or temp directory
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;

    // Canonicalize temp directory to handle symlinks (e.g., /var -> /private/var on macOS)
    let temp_dir = std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir());

    // Allow paths within home directory, current working directory, or temp directory
    let is_in_home = canonical_path.starts_with(&home_dir);
    let is_in_cwd = canonical_path.starts_with(&current_dir);
    let is_in_temp = canonical_path.starts_with(&temp_dir);

    if !is_in_home && !is_in_cwd && !is_in_temp {
        return Err(anyhow::anyhow!(
            "Access denied: path '{}' is outside allowed directories (home, current, or temp directory)",
            canonical_path.display()
        ));
    }

    // Additional check: reject system directories
    let system_dirs = [
        "/etc",
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        "/System",
        "/Library",
    ];
    for sys_dir in &system_dirs {
        if canonical_path.starts_with(sys_dir) {
            return Err(anyhow::anyhow!(
                "Access denied: cannot access system directory '{}'",
                sys_dir
            ));
        }
    }

    Ok(canonical_path)
}

/// Run a shell command (with safety checks)
pub struct RunCommandTool;

#[async_trait]
impl ToolHandler for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Run a shell command safely. Some dangerous commands are blocked for safety."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory (default: current directory)"
                }
            }),
            vec!["command"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;
        let working_dir = input
            .get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        // Maximum command length check
        const MAX_COMMAND_LENGTH: usize = 1000;
        if command.len() > MAX_COMMAND_LENGTH {
            warn!(
                "Blocked command exceeding max length: {} chars",
                command.len()
            );
            return Err(anyhow::anyhow!(
                "Command exceeds maximum length of {} characters",
                MAX_COMMAND_LENGTH
            ));
        }

        // Allowlist of safe commands
        //
        // Security notes — intentionally EXCLUDED:
        //   env, printenv  — leak all env vars including API keys/tokens (C-1)
        //   curl, wget     — enable data exfiltration, bypass SSRF protection (C-2)
        //   osascript       — bypasses browser JS blocklist & AppleScript sanitization (H-2)
        //   python*, node, ruby — arbitrary code execution via interpreters (M-5)
        //   defaults        — can modify macOS system preferences
        const ALLOWED_COMMANDS: &[&str] = &[
            // Read-only / informational
            "ls",
            "cat",
            "head",
            "tail",
            "wc",
            "echo",
            "date",
            "whoami",
            "uname",
            "pwd",
            "which",
            "file",
            "stat",
            "du",
            "df",
            "uptime",
            "ps",
            "hostname",
            "id",
            "groups",
            "grep",
            "find",
            "sort",
            "uniq",
            "cut",
            "awk",
            "sed",
            "tr",
            "basename",
            "dirname",
            "realpath",
            "readlink",
            // File operations (mv removed — can overwrite critical files)
            "mkdir",
            "cp",
            "touch",
            "ln",
            "chmod",
            "tar",
            "zip",
            "unzip",
            "gzip",
            // Networking (read-only diagnostics only)
            "ping",
            "dig",
            "nslookup",
            // Development tools (build tools only, no interpreters)
            "git",
            "npm",
            "npx",
            "cargo",
            "go",
            "pip",
            "pip3",
            "make",
            "cmake",
            "brew",
            // macOS utilities
            "open",
            "pbcopy",
            "pbpaste",
            "say",
        ];

        // Shell metacharacters that allow chaining/redirecting commands.
        // We split on these to extract EVERY command in the pipeline and validate each one.
        const SHELL_CHAIN_CHARS: &[char] = &['|', ';', '&', '\n'];

        // Also block dangerous shell operators that can't be split simply
        let dangerous_operators = ["`", "$(", ">>", ">", "<(", ">("];
        for op in &dangerous_operators {
            if command.contains(op) {
                warn!(
                    "Blocked command containing shell operator '{}': {}",
                    op, command
                );
                return Err(anyhow::anyhow!(
                    "Command blocked: shell operator '{}' is not allowed for security reasons",
                    op
                ));
            }
        }

        // Split on chain characters and validate EVERY command in the pipeline
        let segments: Vec<&str> = command
            .split(SHELL_CHAIN_CHARS)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if segments.is_empty() {
            return Err(anyhow::anyhow!("Empty command"));
        }

        for segment in &segments {
            let first_word = segment.split_whitespace().next().unwrap_or("");

            if !ALLOWED_COMMANDS.contains(&first_word) {
                warn!(
                    "Blocked command not in allowlist: '{}' (in segment: '{}')",
                    first_word, segment
                );
                return Err(anyhow::anyhow!(
                    "Command '{}' is not in the allowlist of safe commands",
                    first_word
                ));
            }
        }

        // Secondary blocklist check for extra safety
        let dangerous_patterns = [
            "rm -rf /",
            "rm -rf /*",
            "sudo rm",
            "mkfs",
            "dd if=",
            ":(){ :|:& };:",
        ];

        for pattern in &dangerous_patterns {
            if command.contains(pattern) {
                warn!("Blocked dangerous command: {}", command);
                return Err(anyhow::anyhow!(
                    "Command blocked for safety: contains '{}'",
                    pattern
                ));
            }
        }

        debug!("Running command: {} (in {})", command, working_dir);

        // Execute with timeout
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(working_dir)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command execution timed out after 30 seconds"))?
        .context("Failed to execute command")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str("STDOUT:\n");
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n\n");
            }
            result.push_str("STDERR:\n");
            result.push_str(&stderr);
        }

        if !output.status.success() {
            result.push_str(&format!(
                "\n\nExit code: {}",
                output.status.code().unwrap_or(-1)
            ));
        }

        Ok(result)
    }
}

/// Read file from disk
pub struct ReadFileTool;

#[async_trait]
impl ToolHandler for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from disk."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            }),
            vec!["path"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        const MAX_READ_SIZE: u64 = 10 * 1024 * 1024; // 10MB

        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        debug!("Reading file: {}", path);

        // Validate path to prevent path traversal
        let validated_path = validate_file_path(path, false)?;

        // Check file size before reading
        let metadata = tokio::fs::metadata(&validated_path)
            .await
            .with_context(|| {
                format!("Failed to read file metadata: {}", validated_path.display())
            })?;

        let file_size = metadata.len();
        if file_size > MAX_READ_SIZE {
            return Err(anyhow::anyhow!(
                "File too large ({} bytes, max 10MB)",
                file_size
            ));
        }

        let content = tokio::fs::read_to_string(&validated_path)
            .await
            .with_context(|| format!("Failed to read file: {}", validated_path.display()))?;

        Ok(content)
    }
}

/// Write file to disk
pub struct WriteFileTool;

#[async_trait]
impl ToolHandler for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file on disk. Creates parent directories if needed."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            }),
            vec!["path", "content"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        const MAX_WRITE_SIZE: usize = 10 * 1024 * 1024; // 10MB

        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        // Check content size before writing
        if content.len() > MAX_WRITE_SIZE {
            return Err(anyhow::anyhow!(
                "Content too large ({} bytes, max 10MB)",
                content.len()
            ));
        }

        debug!("Writing file: {} ({} bytes)", path, content.len());

        // Validate path to prevent path traversal
        let validated_path = validate_file_path(path, true)?;

        // Create parent directories if needed
        if let Some(parent) = validated_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directories")?;
        }

        tokio::fs::write(&validated_path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", validated_path.display()))?;

        Ok(format!(
            "Successfully wrote {} bytes to {}",
            content.len(),
            validated_path.display()
        ))
    }
}

/// Check if an IP address is private/loopback/link-local (unsafe for SSRF)
fn is_private_ip(ip: &std::net::IpAddr) -> Option<&'static str> {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            if octets[0] == 10 {
                Some("private IP range (10.x.x.x)")
            } else if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                Some("private IP range (172.16-31.x.x)")
            } else if octets[0] == 192 && octets[1] == 168 {
                Some("private IP range (192.168.x.x)")
            } else if octets[0] == 169 && octets[1] == 254 {
                Some("link-local address (169.254.x.x)")
            } else if octets[0] == 127 {
                Some("loopback address")
            } else if octets[0] == 0 {
                Some("unspecified address (0.x.x.x)")
            } else {
                None
            }
        }
        IpAddr::V6(ipv6) => {
            if ipv6.is_loopback() {
                Some("IPv6 loopback")
            } else if ipv6.segments()[0] & 0xffc0 == 0xfe80 {
                Some("IPv6 link-local address")
            } else if ipv6.segments()[0] & 0xfe00 == 0xfc00 {
                Some("IPv6 unique local address")
            } else {
                None
            }
        }
    }
}

/// Validated URL info returned by `validate_url`.
/// Contains the resolved IPs so callers can pin them in reqwest,
/// eliminating the TOCTOU gap between DNS check and HTTP request.
struct ValidatedUrl {
    host: String,
    resolved_ips: Vec<std::net::SocketAddr>,
}

/// Check if a URL is safe to fetch (SSRF protection).
///
/// Returns resolved socket addresses so the caller can pin them in the HTTP
/// client, preventing DNS rebinding between validation and the actual request.
fn validate_url(url_str: &str) -> Result<ValidatedUrl> {
    use std::net::IpAddr;

    let url = url::Url::parse(url_str).context("Invalid URL format")?;

    // Only allow HTTP and HTTPS
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(anyhow::anyhow!("Only HTTP and HTTPS schemes are allowed"));
    }

    // Get the host
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL must have a host"))?;

    // Block localhost variations
    let localhost_patterns = ["localhost", "127.0.0.1", "::1", "0.0.0.0", "[::1]"];
    for pattern in &localhost_patterns {
        if host.eq_ignore_ascii_case(pattern) {
            return Err(anyhow::anyhow!("Access to localhost is not allowed"));
        }
    }

    // Check if host is a direct IP address
    if let Ok(ip) = host.parse::<IpAddr>()
        && let Some(reason) = is_private_ip(&ip)
    {
        return Err(anyhow::anyhow!("Access to {} is not allowed", reason));
    }

    let port = url.port_or_known_default().unwrap_or(80);

    // DNS rebinding mitigation: resolve the hostname, validate all resolved IPs,
    // and return them so the caller can pin them in the HTTP client.
    let resolve_target = format!("{}:{}", host, port);
    let resolved: Vec<std::net::SocketAddr> =
        if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&resolve_target) {
            let addrs: Vec<_> = addrs.collect();
            for addr in &addrs {
                if let Some(reason) = is_private_ip(&addr.ip()) {
                    warn!(
                        "DNS rebinding detected: {} resolved to {} ({})",
                        host,
                        addr.ip(),
                        reason
                    );
                    return Err(anyhow::anyhow!(
                        "Access denied: hostname '{}' resolved to {} ({})",
                        host,
                        addr.ip(),
                        reason
                    ));
                }
            }
            addrs
        } else {
            Vec::new()
        };

    Ok(ValidatedUrl {
        host: host.to_string(),
        resolved_ips: resolved,
    })
}

/// Convenience wrapper that discards resolved IPs (for redirect checks, etc.)
fn is_safe_url(url_str: &str) -> Result<()> {
    validate_url(url_str).map(|_| ())
}

/// Fetch URL content — tries Tavily Extract for clean content, falls back to raw fetch
pub struct BrowseUrlTool {
    tavily: Option<Arc<crate::tavily::TavilyClient>>,
}

impl BrowseUrlTool {
    /// Create with Tavily client for clean content extraction
    pub fn with_tavily(client: Arc<crate::tavily::TavilyClient>) -> Self {
        Self {
            tavily: Some(client),
        }
    }

    /// Create without Tavily — raw fetch only
    pub fn new() -> Self {
        Self { tavily: None }
    }
}

impl Default for BrowseUrlTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolHandler for BrowseUrlTool {
    fn name(&self) -> &str {
        "browse_url"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL. Returns clean extracted text when available, otherwise raw HTML."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers to include (only used for raw fetch fallback)"
                }
            }),
            vec!["url"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

        debug!("Fetching URL: {}", url);

        // Validate URL for SSRF protection and resolve DNS once (applies to both paths)
        let validated = validate_url(url)?;

        // Try Tavily Extract first for clean content
        if let Some(tavily) = &self.tavily {
            match tavily.extract(url).await {
                Ok(content) => {
                    debug!("Tavily extract succeeded for {}", url);
                    const MAX_LENGTH: usize = 50000;
                    if content.len() > MAX_LENGTH {
                        return Ok(format!(
                            "{}\n\n[Content truncated at {} chars]",
                            &content[..MAX_LENGTH],
                            MAX_LENGTH
                        ));
                    }
                    return Ok(content);
                }
                Err(e) => {
                    debug!(
                        "Tavily extract failed for {}, falling back to raw fetch: {}",
                        url, e
                    );
                }
            }
        }

        // Fallback: raw fetch with redirect following, pinning resolved IPs
        self.raw_fetch(url, &input, &validated).await
    }
}

impl BrowseUrlTool {
    async fn raw_fetch(&self, url: &str, input: &Value, validated: &ValidatedUrl) -> Result<String> {
        // Pin resolved IPs in the client to prevent DNS rebinding (H-1 fix).
        // This ensures reqwest uses the same IPs we already validated.
        let mut builder = reqwest::Client::builder()
            .user_agent("meepo-agent/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none());

        for addr in &validated.resolved_ips {
            builder = builder.resolve(&validated.host, *addr);
        }

        let client = builder.build().context("Failed to create HTTP client")?;

        // Headers that must not be overridden by user input (M-7 fix)
        const BLOCKED_HEADERS: &[&str] = &[
            "authorization",
            "cookie",
            "host",
            "x-forwarded-for",
            "x-real-ip",
            "proxy-authorization",
            "set-cookie",
        ];

        let mut current_url = url.to_string();
        let mut redirects = 0;
        let max_redirects = 5;

        let response = loop {
            let mut request = client.get(&current_url);

            if let Some(headers) = input.get("headers").and_then(|v| v.as_object()) {
                for (key, value) in headers {
                    if let Some(value_str) = value.as_str() {
                        if key.contains('\r')
                            || key.contains('\n')
                            || value_str.contains('\r')
                            || value_str.contains('\n')
                        {
                            warn!("Skipping header '{}' due to CRLF characters", key);
                            continue;
                        }
                        if BLOCKED_HEADERS.contains(&key.to_lowercase().as_str()) {
                            warn!("Skipping blocked sensitive header '{}'", key);
                            continue;
                        }
                        request = request.header(key, value_str);
                    }
                }
            }

            let resp = request.send().await.context("Failed to fetch URL")?;

            if resp.status().is_redirection() {
                redirects += 1;
                if redirects > max_redirects {
                    return Ok("Too many redirects".to_string());
                }
                if let Some(location) = resp.headers().get("location") {
                    let redirect_url = location
                        .to_str()
                        .map_err(|_| anyhow::anyhow!("Invalid redirect URL"))?;
                    let resolved = if redirect_url.starts_with("http") {
                        redirect_url.to_string()
                    } else {
                        url::Url::parse(&current_url)
                            .and_then(|base| base.join(redirect_url))
                            .map(|u| u.to_string())
                            .unwrap_or_else(|_| {
                                format!(
                                    "{}/{}",
                                    current_url.trim_end_matches('/'),
                                    redirect_url.trim_start_matches('/')
                                )
                            })
                    };
                    if is_safe_url(&resolved).is_err() {
                        return Ok(format!("Blocked redirect to unsafe URL: {}", resolved));
                    }
                    current_url = resolved;
                    continue;
                } else {
                    return Ok("Redirect without Location header".to_string());
                }
            }
            break resp;
        };

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "HTTP request failed with status: {}",
                status
            ));
        }

        let content = response
            .text()
            .await
            .context("Failed to read response body")?;

        const MAX_LENGTH: usize = 50000;
        if content.len() > MAX_LENGTH {
            Ok(format!(
                "{}\n\n[Content truncated at {} chars]",
                &content[..MAX_LENGTH],
                MAX_LENGTH
            ))
        } else {
            Ok(content)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;
    use tempfile::TempDir;

    #[test]
    fn test_run_command_schema() {
        let tool = RunCommandTool;
        assert_eq!(tool.name(), "run_command");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_read_file_schema() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
    }

    #[test]
    fn test_write_file_schema() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");
    }

    #[test]
    fn test_browse_url_schema() {
        let tool = BrowseUrlTool::new();
        assert_eq!(tool.name(), "browse_url");
    }

    #[tokio::test]
    async fn test_run_command_echo() {
        let tool = RunCommandTool;
        let result = tool
            .execute(serde_json::json!({
                "command": "echo hello_meepo_test"
            }))
            .await
            .unwrap();
        assert!(result.contains("hello_meepo_test"));
    }

    #[tokio::test]
    async fn test_run_command_missing_param() {
        let tool = RunCommandTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_command_blocks_dangerous() {
        let tool = RunCommandTool;
        let result = tool
            .execute(serde_json::json!({
                "command": "rm -rf /"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_command_blocks_not_allowlisted() {
        let tool = RunCommandTool;
        // nc (netcat) is not in the allowlist
        let result = tool
            .execute(serde_json::json!({
                "command": "nc -l 1234"
            }))
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not in the allowlist")
        );
    }

    #[tokio::test]
    async fn test_run_command_blocks_too_long() {
        let tool = RunCommandTool;
        let long_command = "echo ".to_string() + &"A".repeat(1001);
        let result = tool
            .execute(serde_json::json!({
                "command": long_command
            }))
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds maximum length")
        );
    }

    #[tokio::test]
    async fn test_run_command_safe_command_works() {
        let tool = RunCommandTool;
        let result = tool
            .execute(serde_json::json!({
                "command": "ls -la"
            }))
            .await;
        // ls should be allowed and work
        assert!(
            result.is_ok()
                || result
                    .unwrap_err()
                    .to_string()
                    .contains("Failed to execute")
        );
    }

    #[tokio::test]
    async fn test_write_and_read_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        let write_tool = WriteFileTool;
        let result = write_tool
            .execute(serde_json::json!({
                "path": path_str,
                "content": "hello from meepo"
            }))
            .await
            .unwrap();
        assert!(result.contains("Wrote") || result.contains("wrote") || result.contains("bytes"));

        let read_tool = ReadFileTool;
        let result = read_tool
            .execute(serde_json::json!({
                "path": path_str
            }))
            .await
            .unwrap();
        assert_eq!(result.trim(), "hello from meepo");
    }

    #[tokio::test]
    async fn test_read_file_missing() {
        let tool = ReadFileTool;
        let result = tool
            .execute(serde_json::json!({
                "path": "/tmp/nonexistent_meepo_test_file_xyz"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_missing_param() {
        let tool = ReadFileTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_size_limit() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("large.txt");
        let path_str = path.to_str().unwrap();

        // Create a file larger than 10MB
        let large_content = "A".repeat(11 * 1024 * 1024); // 11MB
        std::fs::write(&path, large_content).unwrap();

        let tool = ReadFileTool;
        let result = tool
            .execute(serde_json::json!({
                "path": path_str
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[tokio::test]
    async fn test_write_file_size_limit() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("large.txt");
        let path_str = path.to_str().unwrap();

        // Try to write a file larger than 10MB
        let large_content = "A".repeat(11 * 1024 * 1024); // 11MB

        let tool = WriteFileTool;
        let result = tool
            .execute(serde_json::json!({
                "path": path_str,
                "content": large_content
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[tokio::test]
    async fn test_read_file_path_traversal_blocked() {
        let tool = ReadFileTool;

        // Try to read /etc/passwd using path traversal
        let result = tool
            .execute(serde_json::json!({
                "path": "../../../etc/passwd"
            }))
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("..") || err_msg.contains("not allowed") || err_msg.contains("denied")
        );
    }

    #[tokio::test]
    async fn test_write_file_path_traversal_blocked() {
        let tool = WriteFileTool;

        // Try to write to /etc using path traversal
        let result = tool
            .execute(serde_json::json!({
                "path": "../../../etc/malicious.txt",
                "content": "test"
            }))
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("..") || err_msg.contains("not allowed") || err_msg.contains("denied")
        );
    }

    #[tokio::test]
    async fn test_read_file_normal_path_works() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        // Create a test file
        std::fs::write(&path, "test content").unwrap();

        let tool = ReadFileTool;
        let result = tool
            .execute(serde_json::json!({
                "path": path_str
            }))
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().trim(), "test content");
    }

    #[test]
    fn test_validate_file_path_rejects_dotdot() {
        let result = validate_file_path("../../../etc/passwd", false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(".."));
    }

    #[test]
    fn test_validate_file_path_rejects_system_dirs() {
        // These tests may fail if the paths don't exist, which is fine
        // The important thing is that IF they exist, they should be blocked

        let system_paths = vec!["/etc/test", "/bin/test", "/sbin/test"];

        for path in system_paths {
            // We expect either "denied" or a canonicalization error
            // Both are acceptable outcomes for security
            let result = validate_file_path(path, false);
            if result.is_ok() {
                // If somehow it succeeded, make sure it's not actually in a system dir
                let validated = result.unwrap();
                assert!(!validated.starts_with("/etc"));
                assert!(!validated.starts_with("/bin"));
                assert!(!validated.starts_with("/sbin"));
            }
        }
    }

    #[test]
    fn test_is_safe_url_blocks_localhost() {
        let localhost_urls = vec![
            "http://localhost/api",
            "http://127.0.0.1/api",
            "http://0.0.0.0/api",
            "http://[::1]/api",
        ];

        for url in localhost_urls {
            let result = is_safe_url(url);
            assert!(result.is_err(), "Should block localhost URL: {}", url);
            let err_msg = result.unwrap_err().to_string().to_lowercase();
            assert!(err_msg.contains("localhost") || err_msg.contains("loopback"));
        }
    }

    #[test]
    fn test_is_safe_url_blocks_private_ips() {
        let private_urls = vec![
            "http://10.0.0.1/api",
            "http://192.168.1.1/api",
            "http://172.16.0.1/api",
            "http://172.31.255.255/api",
            "http://169.254.1.1/api",
        ];

        for url in private_urls {
            let result = is_safe_url(url);
            assert!(result.is_err(), "Should block private IP URL: {}", url);
            let err_msg = result.unwrap_err().to_string().to_lowercase();
            assert!(
                err_msg.contains("private")
                    || err_msg.contains("link-local")
                    || err_msg.contains("not allowed")
            );
        }
    }

    #[test]
    fn test_is_safe_url_allows_public() {
        let public_urls = vec![
            "https://www.google.com",
            "https://example.com/api",
            "http://8.8.8.8",
        ];

        for url in public_urls {
            let result = is_safe_url(url);
            assert!(result.is_ok(), "Should allow public URL: {}", url);
        }
    }

    #[test]
    fn test_is_safe_url_blocks_non_http() {
        let non_http_urls = vec![
            "file:///etc/passwd",
            "ftp://example.com",
            "javascript:alert(1)",
        ];

        for url in non_http_urls {
            let result = is_safe_url(url);
            assert!(result.is_err(), "Should block non-HTTP URL: {}", url);
        }
    }

    #[tokio::test]
    async fn test_browse_url_blocks_localhost() {
        let tool = BrowseUrlTool::new();

        let result = tool
            .execute(serde_json::json!({
                "url": "http://localhost:8080/admin"
            }))
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(err_msg.contains("localhost") || err_msg.contains("not allowed"));
    }

    #[tokio::test]
    async fn test_browse_url_blocks_private_ip() {
        let tool = BrowseUrlTool::new();

        let result = tool
            .execute(serde_json::json!({
                "url": "http://192.168.1.1/router"
            }))
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(err_msg.contains("private") || err_msg.contains("not allowed"));
    }
}
