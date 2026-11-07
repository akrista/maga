use crate::domain::model::{McpServerConfig, Transport};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Default timeout for a live MCP health probe (`initialize` + `tools/list`).
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// Metadata for a single tool exposed by an MCP server via `tools/list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
}

/// Completed outcome of a live MCP server probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub latency_ms: u64,
    pub server_info: Option<String>,
    pub protocol_version: Option<String>,
    pub tools: Vec<McpToolInfo>,
}

/// Live health probe state tracked per MCP server in `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerProbeState {
    Probing,
    Online(ProbeOutcome),
    Failed { error: String, latency_ms: u64 },
}

/// Message sent from a background probe worker thread back to the TUI event loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeEvent {
    pub name: String,
    pub state: ServerProbeState,
}

/// Spawns a detached background thread to probe `cfg` and send the resulting `ProbeEvent` to `tx`.
pub fn spawn_probe_worker(name: String, cfg: McpServerConfig, tx: mpsc::Sender<ProbeEvent>) {
    thread::spawn(move || {
        let state = probe_server(&cfg, DEFAULT_PROBE_TIMEOUT);
        let _ = tx.send(ProbeEvent { name, state });
    });
}

/// Executes a synchronous MCP `initialize` + `tools/list` probe within `timeout`.
pub fn probe_server(cfg: &McpServerConfig, timeout: Duration) -> ServerProbeState {
    let start = Instant::now();
    let res = match cfg.transport {
        Transport::Stdio => probe_stdio_server(cfg, timeout),
        Transport::Http => probe_http_server(cfg, timeout),
    };
    let latency_ms = start.elapsed().as_millis() as u64;
    match res {
        Ok(mut outcome) => {
            outcome.latency_ms = latency_ms;
            ServerProbeState::Online(outcome)
        }
        Err(error) => ServerProbeState::Failed { error, latency_ms },
    }
}

pub fn build_initialize_request(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "maga",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    })
}

pub fn build_initialized_notification() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })
}

pub fn build_tools_list_request(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/list",
        "params": {}
    })
}

/// Parses a JSON-RPC response body that may be either raw `application/json` or `text/event-stream` (SSE) `data:` frames.
pub fn parse_jsonrpc_http_body(body: &str, expected_id: u64) -> Result<Value, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("Empty response body from MCP server".to_string());
    }

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let val: Value =
            serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON response: {e}"))?;
        if let Some(arr) = val.as_array() {
            for item in arr {
                if matches_jsonrpc_id(item, expected_id) {
                    return Ok(item.clone());
                }
            }
            return arr
                .first()
                .cloned()
                .ok_or_else(|| "Empty JSON-RPC batch array".to_string());
        }
        return Ok(val);
    }

    // Parse SSE `data:` lines
    let mut last_json: Option<Value> = None;
    for line in trimmed.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("data:") {
            let data = rest.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(data) {
                if matches_jsonrpc_id(&val, expected_id) {
                    return Ok(val);
                }
                last_json = Some(val);
            }
        }
    }

    last_json.ok_or_else(|| {
        let snippet: String = trimmed.chars().take(120).collect();
        format!("Unrecognized MCP HTTP response: {snippet}")
    })
}

fn matches_jsonrpc_id(val: &Value, expected_id: u64) -> bool {
    match val.get("id") {
        Some(Value::Number(n)) => n.as_u64() == Some(expected_id),
        Some(Value::String(s)) => s == &expected_id.to_string(),
        _ => false,
    }
}

/// Extracts `result` or formats `error` from a JSON-RPC response object.
pub fn extract_jsonrpc_result(msg: &Value) -> Result<&Value, String> {
    if let Some(err) = msg.get("error") {
        let code = err.get("code").and_then(Value::as_i64);
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("JSON-RPC error");
        return match code {
            Some(c) => Err(format!("RPC error {c}: {message}")),
            None => Err(format!("RPC error: {message}")),
        };
    }
    msg.get("result")
        .ok_or_else(|| "Missing `result` in JSON-RPC response".to_string())
}

/// Extracts `(server_info, protocol_version)` from an `initialize` result payload.
pub fn parse_initialize_result(result: &Value) -> (Option<String>, Option<String>) {
    let protocol_version = result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let server_info = result.get("serverInfo").and_then(|info| {
        let name = info.get("name").and_then(Value::as_str)?;
        let version = info.get("version").and_then(Value::as_str);
        match version {
            Some(v) if !v.trim().is_empty() => Some(format!("{name} v{v}")),
            _ => Some(name.to_string()),
        }
    });
    (server_info, protocol_version)
}

/// Extracts `Vec<McpToolInfo>` from a `tools/list` result payload.
pub fn parse_tools_list_result(result: &Value) -> Vec<McpToolInfo> {
    let Some(arr) = result.get("tools").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut tools = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty());
        tools.push(McpToolInfo {
            name: name.to_string(),
            description,
        });
    }
    tools
}

/// Probes a `stdio` MCP server by spawning its command, performing the `initialize` + `tools/list`
/// handshake over `stdin`/`stdout` within `timeout`, and unconditionally killing/reaping the child.
pub fn probe_stdio_server(
    cfg: &McpServerConfig,
    timeout: Duration,
) -> Result<ProbeOutcome, String> {
    let cmd_str = cfg.command.as_deref().unwrap_or("").trim();
    if cmd_str.is_empty() {
        return Err("Stdio server has no command configured".to_string());
    }

    let mut child =
        spawn_stdio_child(cmd_str, cfg).map_err(|e| format!("Failed to spawn '{cmd_str}': {e}"))?;

    let deadline = Instant::now() + timeout;
    let res = run_stdio_handshake(&mut child, deadline);

    // Always terminate and reap the child process tree so no orphans remain
    kill_and_reap_child(&mut child);

    res
}

fn spawn_stdio_child(cmd_str: &str, cfg: &McpServerConfig) -> std::io::Result<Child> {
    #[cfg(windows)]
    {
        let lower = cmd_str.to_ascii_lowercase();
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            return Command::new("cmd")
                .arg("/C")
                .arg(cmd_str)
                .args(&cfg.args)
                .envs(&cfg.env)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
        }
    }

    let direct = Command::new(cmd_str)
        .args(&cfg.args)
        .envs(&cfg.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    #[cfg(windows)]
    if let Err(ref e) = direct
        && e.kind() == std::io::ErrorKind::NotFound
    {
        return Command::new("cmd")
            .arg("/C")
            .arg(cmd_str)
            .args(&cfg.args)
            .envs(&cfg.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
    }

    direct
}

fn kill_and_reap_child(child: &mut Child) {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run_stdio_handshake(child: &mut Child, deadline: Instant) -> Result<ProbeOutcome, String> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open child stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to open child stdout".to_string())?;
    let stderr = child.stderr.take();

    let (line_tx, line_rx) = mpsc::channel::<Result<String, String>>();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if line_tx.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = line_tx.send(Err(format!("stdout read error: {e}")));
                    break;
                }
            }
        }
    });

    let (err_tx, err_rx) = mpsc::channel::<String>();
    if let Some(mut err_stream) = stderr {
        thread::spawn(move || {
            let mut buf = [0u8; 2048];
            if let Ok(n) = err_stream.read(&mut buf)
                && n > 0
            {
                let text = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                if !text.is_empty() {
                    let _ = err_tx.send(text);
                }
            }
        });
    }

    // 1. Send `initialize` (id = 1)
    let _ = send_stdio_json(&mut stdin, &build_initialize_request(1));
    let init_msg = recv_stdio_jsonrpc(&line_rx, &err_rx, 1, deadline, "initialize")?;
    let init_result = extract_jsonrpc_result(&init_msg)?;
    let (server_info, protocol_version) = parse_initialize_result(init_result);

    // 2. Send `notifications/initialized`
    let _ = send_stdio_json(&mut stdin, &build_initialized_notification());

    // 3. Send `tools/list` (id = 2)
    let _ = send_stdio_json(&mut stdin, &build_tools_list_request(2));
    let tools_msg = recv_stdio_jsonrpc(&line_rx, &err_rx, 2, deadline, "tools/list")?;
    let tools_result = extract_jsonrpc_result(&tools_msg)?;
    let tools = parse_tools_list_result(tools_result);

    Ok(ProbeOutcome {
        latency_ms: 0,
        server_info,
        protocol_version,
        tools,
    })
}

fn send_stdio_json(stdin: &mut impl Write, payload: &Value) -> Result<(), String> {
    let serialized =
        serde_json::to_string(payload).map_err(|e| format!("JSON serialize error: {e}"))?;
    writeln!(stdin, "{serialized}").map_err(|e| format!("Failed to write to child stdin: {e}"))?;
    stdin
        .flush()
        .map_err(|e| format!("Failed to flush child stdin: {e}"))
}

fn recv_stdio_jsonrpc(
    line_rx: &mpsc::Receiver<Result<String, String>>,
    err_rx: &mpsc::Receiver<String>,
    expected_id: u64,
    deadline: Instant,
    step: &str,
) -> Result<Value, String> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!("Timed out waiting for `{step}` response"));
        }
        let remaining = deadline.saturating_duration_since(now);
        match line_rx.recv_timeout(remaining) {
            Ok(Ok(line)) => {
                let trimmed = line.trim();
                if !trimmed.starts_with('{') {
                    // Ignore non-JSON startup banners on stdout
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed)
                    && matches_jsonrpc_id(&val, expected_id)
                {
                    return Ok(val);
                }
            }
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(format!("Timed out waiting for `{step}` response"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Ok(stderr_msg) = err_rx.recv_timeout(Duration::from_millis(80)) {
                    let first_line = stderr_msg.lines().next().unwrap_or(&stderr_msg);
                    return Err(format!(
                        "Process exited before `{step}` response: {first_line}"
                    ));
                }
                return Err(format!(
                    "Process closed stdout before responding to `{step}`"
                ));
            }
        }
    }
}

/// Probes an `http` MCP server using `ureq`, sending custom headers, propagating `Mcp-Session-Id`,
/// and parsing either `application/json` or `text/event-stream` (SSE) responses.
pub fn probe_http_server(cfg: &McpServerConfig, timeout: Duration) -> Result<ProbeOutcome, String> {
    let url = cfg.url.as_deref().unwrap_or("").trim();
    if url.is_empty() {
        return Err("HTTP server has no URL configured".to_string());
    }

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build();

    // 1. Send `initialize` (id = 1)
    let (init_body, session_id) = send_http_jsonrpc(
        &agent,
        url,
        &cfg.headers,
        None,
        &build_initialize_request(1),
    )?;
    let init_msg = parse_jsonrpc_http_body(&init_body, 1)?;
    let init_result = extract_jsonrpc_result(&init_msg)?;
    let (server_info, protocol_version) = parse_initialize_result(init_result);

    // 2. Best-effort `notifications/initialized`
    let _ = send_http_jsonrpc(
        &agent,
        url,
        &cfg.headers,
        session_id.as_deref(),
        &build_initialized_notification(),
    );

    // 3. Send `tools/list` (id = 2)
    let (tools_body, _) = send_http_jsonrpc(
        &agent,
        url,
        &cfg.headers,
        session_id.as_deref(),
        &build_tools_list_request(2),
    )?;
    let tools_msg = parse_jsonrpc_http_body(&tools_body, 2)?;
    let tools_result = extract_jsonrpc_result(&tools_msg)?;
    let tools = parse_tools_list_result(tools_result);

    Ok(ProbeOutcome {
        latency_ms: 0,
        server_info,
        protocol_version,
        tools,
    })
}

fn send_http_jsonrpc(
    agent: &ureq::Agent,
    url: &str,
    headers: &std::collections::BTreeMap<String, String>,
    session_id: Option<&str>,
    payload: &Value,
) -> Result<(String, Option<String>), String> {
    let mut req = agent
        .post(url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream");

    for (k, v) in headers {
        req = req.set(k, v);
    }
    if let Some(sid) = session_id {
        req = req.set("Mcp-Session-Id", sid);
    }

    match req.send_json(payload.clone()) {
        Ok(resp) => {
            let sid = resp
                .header("Mcp-Session-Id")
                .or_else(|| resp.header("mcp-session-id"))
                .map(ToString::to_string);
            let body = resp
                .into_string()
                .map_err(|e| format!("Failed reading HTTP response body: {e}"))?;
            Ok((body, sid))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let detail = body.lines().next().unwrap_or("").trim();
            if detail.is_empty() {
                Err(format!("HTTP {code}"))
            } else {
                let snippet: String = detail.chars().take(100).collect();
                Err(format!("HTTP {code}: {snippet}"))
            }
        }
        Err(ureq::Error::Transport(t)) => Err(format!("HTTP transport error: {t}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parses_json_and_sse_jsonrpc_bodies_and_tools() {
        let raw_json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "context7", "version": "1.2.0" }
            }
        }"#;
        let msg = parse_jsonrpc_http_body(raw_json, 1).expect("parse json");
        let res = extract_jsonrpc_result(&msg).expect("extract result");
        let (info, proto) = parse_initialize_result(res);
        assert_eq!(info.as_deref(), Some("context7 v1.2.0"));
        assert_eq!(proto.as_deref(), Some("2024-11-05"));

        let sse_body = "\
event: message\n\
data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\"}\n\n\
event: message\n\
data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"resolve-library-id\",\"description\":\"Resolves a package\\n  name\"},{\"name\":\"query-docs\"}]}}\n\n";
        let tools_msg = parse_jsonrpc_http_body(sse_body, 2).expect("parse sse");
        let tools_res = extract_jsonrpc_result(&tools_msg).expect("extract tools result");
        let tools = parse_tools_list_result(tools_res);
        assert_eq!(
            tools,
            vec![
                McpToolInfo {
                    name: "resolve-library-id".to_string(),
                    description: Some("Resolves a package name".to_string()),
                },
                McpToolInfo {
                    name: "query-docs".to_string(),
                    description: None,
                },
            ]
        );
    }

    #[test]
    fn probes_stdio_server_with_banner_and_extracts_tools() {
        #[cfg(windows)]
        let (_dir, cfg) = {
            let dir = tempfile::tempdir().expect("tempdir");
            let cmd_path = dir.path().join("mock-mcp.cmd");
            let script = "@echo off\r\n\
echo Starting custom MCP server banner...\r\n\
set /p line1=\r\n\
echo {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2024-11-05\",\"serverInfo\":{\"name\":\"mock-stdio\",\"version\":\"0.4.2\"}}}\r\n\
set /p line2=\r\n\
echo {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"read_file\",\"description\":\"Reads a file from disk\"},{\"name\":\"list_dir\"}]}}\r\n\
ping -n 30 127.0.0.1 >nul\r\n";
            std::fs::write(&cmd_path, script).expect("write mock-mcp.cmd");
            // Pass path without .cmd extension to also verify Windows command shim resolution via `cmd /C`
            let shim_no_ext = dir.path().join("mock-mcp");
            (
                dir,
                McpServerConfig::new_stdio(shim_no_ext.to_string_lossy().to_string(), vec![]),
            )
        };

        #[cfg(not(windows))]
        let cfg = {
            let script = r#"
echo "Starting custom MCP server banner..."
read line1
echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"mock-stdio","version":"0.4.2"}}}'
read line2
read line3
echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"read_file","description":"Reads a file from disk"},{"name":"list_dir"}]}}'
sleep 30
"#;
            McpServerConfig::new_stdio("sh", vec!["-c".to_string(), script.to_string()])
        };

        let state = probe_server(&cfg, Duration::from_secs(3));
        match state {
            ServerProbeState::Online(outcome) => {
                assert_eq!(outcome.server_info.as_deref(), Some("mock-stdio v0.4.2"));
                assert_eq!(outcome.protocol_version.as_deref(), Some("2024-11-05"));
                assert_eq!(outcome.tools.len(), 2);
                assert_eq!(outcome.tools[0].name, "read_file");
                assert_eq!(
                    outcome.tools[0].description.as_deref(),
                    Some("Reads a file from disk")
                );
                assert_eq!(outcome.tools[1].name, "list_dir");
            }
            other => panic!("Expected Online, got {other:?}"),
        }
    }

    #[test]
    fn stdio_probe_handles_process_error_and_timeout_cleanly() {
        #[cfg(windows)]
        let fail_cfg = McpServerConfig::new_stdio(
            "cmd",
            vec![
                "/C".to_string(),
                "echo missing API_KEY 1>&2 & exit /b 1".to_string(),
            ],
        );
        #[cfg(not(windows))]
        let fail_cfg = McpServerConfig::new_stdio(
            "sh",
            vec![
                "-c".to_string(),
                "echo 'missing API_KEY' >&2; exit 1".to_string(),
            ],
        );

        match probe_server(&fail_cfg, Duration::from_secs(2)) {
            ServerProbeState::Failed { error, .. } => {
                assert!(
                    error.contains("missing API_KEY") || error.contains("closed stdout"),
                    "unexpected error: {error}"
                );
            }
            other => panic!("Expected Failed, got {other:?}"),
        }

        #[cfg(windows)]
        let hang_cfg = McpServerConfig::new_stdio(
            "cmd",
            vec!["/C".to_string(), "ping -n 15 127.0.0.1 >nul".to_string()],
        );
        #[cfg(not(windows))]
        let hang_cfg =
            McpServerConfig::new_stdio("sh", vec!["-c".to_string(), "sleep 10".to_string()]);

        let start = Instant::now();
        match probe_server(&hang_cfg, Duration::from_millis(200)) {
            ServerProbeState::Failed { error, .. } => {
                assert!(error.contains("Timed out"), "unexpected error: {error}");
                assert!(start.elapsed() < Duration::from_secs(2));
            }
            other => panic!("Expected timeout Failed, got {other:?}"),
        }
    }

    #[test]
    fn probes_http_server_with_custom_headers_session_id_and_sse() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");

        let server_handle = thread::spawn(move || {
            // Handle 3 requests: initialize, notifications/initialized, tools/list
            for req_idx in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept connection");
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut headers_raw = Vec::new();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("read header line");
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_len = rest.trim().parse().unwrap_or(0);
                    }
                    headers_raw.push(line);
                }
                let mut body_buf = vec![0u8; content_len];
                reader.read_exact(&mut body_buf).expect("read body");
                let headers_joined = headers_raw.join("").to_ascii_lowercase();
                assert!(headers_joined.contains("x-api-key: secret-token"));

                if req_idx == 0 {
                    let resp = concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "Content-Type: application/json\r\n",
                        "Mcp-Session-Id: sess-42\r\n",
                        "Connection: close\r\n\r\n",
                        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"http-mcp","version":"2.0"}}}"#
                    );
                    stream.write_all(resp.as_bytes()).expect("write init resp");
                } else if req_idx == 1 {
                    assert!(headers_joined.contains("mcp-session-id: sess-42"));
                    let resp = "HTTP/1.1 202 Accepted\r\nConnection: close\r\n\r\n{}";
                    stream.write_all(resp.as_bytes()).expect("write notif resp");
                } else {
                    assert!(headers_joined.contains("mcp-session-id: sess-42"));
                    let resp = concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "Content-Type: text/event-stream\r\n",
                        "Connection: close\r\n\r\n",
                        "event: message\n",
                        "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"search_web\",\"description\":\"Search the web\"}]}}\n\n"
                    );
                    stream.write_all(resp.as_bytes()).expect("write tools resp");
                }
            }
        });

        let mut cfg = McpServerConfig::new_http(format!("http://{addr}/mcp"));
        cfg.headers
            .insert("X-Api-Key".to_string(), "secret-token".to_string());

        let state = probe_server(&cfg, Duration::from_secs(3));
        server_handle.join().expect("join mock server");

        match state {
            ServerProbeState::Online(outcome) => {
                assert_eq!(outcome.server_info.as_deref(), Some("http-mcp v2.0"));
                assert_eq!(outcome.tools.len(), 1);
                assert_eq!(outcome.tools[0].name, "search_web");
                assert_eq!(
                    outcome.tools[0].description.as_deref(),
                    Some("Search the web")
                );
            }
            other => panic!("Expected Online, got {other:?}"),
        }
    }
}
