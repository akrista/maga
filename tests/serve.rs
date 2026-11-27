//! Integration tests for `maga --serve`: spawns the real binary and drives a full MCP session.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(15);

struct Server {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
    _home: tempfile::TempDir,
}

impl Server {
    fn send(&mut self, frame: &Value) {
        writeln!(self.stdin, "{frame}").expect("write frame to child stdin");
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").expect("write raw line to child stdin");
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.recv_response(id)
    }

    fn recv_response(&self, id: u64) -> Value {
        loop {
            let line = self.rx.recv_timeout(TIMEOUT).expect("child response");
            let frame: Value = serde_json::from_str(&line).expect("valid JSON frame");
            if frame.get("id") == Some(&json!(id)) {
                break frame;
            }
        }
    }

    /// Tool-call helper returning the frame and raw text content.
    fn call_tool(&mut self, id: u64, name: &str, arguments: Value) -> (Value, String) {
        let frame = self.request(
            id,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let text = frame["result"]["content"][0]["text"]
            .as_str()
            .expect("text content")
            .to_string();
        (frame, text)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_server(mutations: bool) -> Server {
    let home = tempfile::TempDir::new().expect("tempdir");
    // Override every storage path into the temp home: on Windows the process home is
    // not redirected by HOME/XDG env vars, and a partial override would let sync and
    // toggle touch the real harness configs.
    let cfg_path = |rel: &str| home.path().join(rel).to_string_lossy().into_owned();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_maga"));
    cmd.arg("--serve")
        .args([
            "--config",
            cfg_path("config/maga/config.toml").as_str(),
            "--mcp-config",
            cfg_path("config/maga/mcp.toml").as_str(),
            "--plugins-config",
            cfg_path("config/maga/plugins.toml").as_str(),
            "--agy-config",
            cfg_path("gemini/mcp_config.json").as_str(),
            "--claude-config",
            cfg_path("claude.json").as_str(),
            "--copilot-config",
            cfg_path("copilot/mcp-config.json").as_str(),
            "--grok-config",
            cfg_path("grok/config.toml").as_str(),
            "--opencode-config",
            cfg_path("opencode/opencode.json").as_str(),
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env_remove("MAGA_MCP_MUTATIONS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if mutations {
        cmd.env("MAGA_MCP_MUTATIONS", "1");
    }
    let mut child = cmd.spawn().expect("spawn maga --serve");
    let stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    Server {
        child,
        stdin,
        rx,
        _home: home,
    }
}

/// Writes a canonical registry with one stdio server whose binary does not exist,
/// so probes fail fast without touching the network.
fn seed_registry(home: &std::path::Path) {
    let dir = home.join("config/maga");
    std::fs::create_dir_all(&dir).expect("create maga config dir");
    std::fs::write(
        dir.join("mcp.toml"),
        "[mcps.ctx7]\n\
         enabled = true\n\
         transport = \"stdio\"\n\
         command = \"definitely-not-a-real-maga-test-binary\"\n\
         description = \"Test server\"\n",
    )
    .expect("write mcp.toml");
}

#[test]
fn full_session_with_mutations() {
    let mut server = spawn_server(true);
    seed_registry(server._home.path());

    let init = server.request(
        1,
        "initialize",
        json!({ "protocolVersion": "2024-11-05", "capabilities": {} }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(init["result"]["serverInfo"]["name"], "maga");

    // The initialized notification must never be answered.
    server.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        matches!(server.rx.try_recv(), Err(TryRecvError::Empty)),
        "notification was answered"
    );

    let ping = server.request(2, "ping", json!({}));
    assert_eq!(ping["result"], json!({}));

    let tools = server.request(3, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "list_servers",
            "probe_server",
            "check_drift",
            "list_harnesses",
            "list_plugins",
            "list_secrets",
            "list_skills",
            "search_skills",
            "toggle_server",
            "sync",
            "set_server_harness",
            "install_skill",
            "set_skill_harness"
        ]
    );

    let (_, text) = server.call_tool(4, "list_servers", json!({}));
    let list: Value = serde_json::from_str(&text).expect("JSON payload");
    assert_eq!(list["servers"][0]["name"], "ctx7");
    assert_eq!(list["servers"][0]["enabled"], true);
    assert_eq!(list["conflict_count"], 0);

    let (_, text) = server.call_tool(5, "probe_server", json!({}));
    let probe: Value = serde_json::from_str(&text).expect("JSON payload");
    assert_eq!(probe["results"][0]["name"], "ctx7");
    assert_eq!(probe["results"][0]["status"], "failed");

    let (_, text) = server.call_tool(6, "check_drift", json!({}));
    let drift: Value = serde_json::from_str(&text).expect("JSON payload");
    assert_eq!(drift["status"], "drift");

    let (frame, _) = server.call_tool(
        7,
        "toggle_server",
        json!({ "name": "nope", "enabled": true }),
    );
    assert_eq!(frame["result"]["isError"], true);
    assert!(
        frame["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Unknown server 'nope'")
    );

    let (frame, text) = server.call_tool(
        8,
        "toggle_server",
        json!({ "name": "ctx7", "enabled": false }),
    );
    assert_eq!(frame["result"]["isError"], false);
    let toggle: Value = serde_json::from_str(&text).expect("JSON payload");
    assert_eq!(toggle["ok"], true);
    assert!(!toggle["notes"].as_array().expect("notes").is_empty());
    let mcp_toml = std::fs::read_to_string(server._home.path().join("config/maga/mcp.toml"))
        .expect("read mcp.toml");
    assert!(
        mcp_toml.contains("enabled = false"),
        "persisted toggle, got: {mcp_toml}"
    );

    let (_, text) = server.call_tool(9, "sync", json!({ "force": true }));
    let synced: Value = serde_json::from_str(&text).expect("JSON payload");
    assert_eq!(synced["ok"], true);
    assert_eq!(synced["synced"], true);
}

#[test]
fn survives_client_dropping_stderr() {
    // Replicates the self-probe failure: a minimal client reads one stderr chunk,
    // drops the server's stderr pipe, and keeps talking protocol on stdout. The
    // server must tolerate the broken stderr instead of panicking (exit code 101).
    let home = tempfile::TempDir::new().expect("tempdir");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_maga"));
    cmd.arg("--serve")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env_remove("MAGA_MCP_MUTATIONS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn maga --serve");
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut stderr = child.stderr.take().expect("child stderr");

    let mut banner = [0u8; 2048];
    let n = stderr.read(&mut banner).expect("read stderr chunk");
    assert!(n > 0, "expected startup banner on stderr");
    drop(stderr);

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2024-11-05","capabilities":{{}},"clientInfo":{{"name":"t","version":"0"}}}}}}"#
    )
    .expect("write initialize");
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .expect("write notification");
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{}}}}"#
    )
    .expect("write tools/list");

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("initialize response");
    assert!(
        line.contains("\"id\":1"),
        "initialize response, got: {line}"
    );

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("tools/list response — child died after stderr was dropped");
    assert!(
        line.contains("\"id\":2"),
        "tools/list response, got: {line}"
    );
    assert!(line.contains("list_servers"), "tools payload, got: {line}");

    assert!(
        child.try_wait().expect("child status").is_none(),
        "server exited before stdin closed"
    );
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn read_only_session() {
    let mut server = spawn_server(false);

    let tools = server.request(1, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "list_servers",
            "probe_server",
            "check_drift",
            "list_harnesses",
            "list_plugins",
            "list_secrets",
            "list_skills",
            "search_skills"
        ]
    );

    let (frame, _) = server.call_tool(
        2,
        "toggle_server",
        json!({ "name": "ctx7", "enabled": true }),
    );
    assert_eq!(frame["result"]["isError"], true);
    assert!(
        frame["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("read-only")
    );
    assert!(
        !server._home.path().join("config/maga/mcp.toml").exists(),
        "read-only mode must not write mcp.toml"
    );

    let unknown = server.request(3, "nope/nope", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);

    server.send_raw("{not json");
    let frame = server
        .rx
        .recv_timeout(TIMEOUT)
        .expect("parse error response");
    let frame: Value = serde_json::from_str(&frame).expect("valid JSON frame");
    assert_eq!(frame["error"]["code"], -32700);
    assert_eq!(frame["id"], Value::Null);

    // The server survives protocol errors.
    let ping = server.request(4, "ping", json!({}));
    assert_eq!(ping["result"], json!({}));
}

#[test]
fn new_tools_over_the_real_binary_never_leak_secrets() {
    let mut server = spawn_server(false);
    let dir = server._home.path().join("config/maga");
    std::fs::create_dir_all(&dir).expect("create maga config dir");
    std::fs::write(
        dir.join("mcp.toml"),
        "[mcps.api]\n\
         enabled = true\n\
         transport = \"http\"\n\
         url = \"https://api.example.com/mcp?apiKey=sk-live-SECRETVALUE99\"\n\
         [mcps.api.headers]\n\
         Authorization = \"Bearer sk-live-SECRETVALUE99\"\n",
    )
    .expect("write mcp.toml");
    std::fs::write(
        dir.join(".env.local"),
        "OTHER_TOKEN=\"another-secret-value-7\"\n",
    )
    .expect("write .env.local");

    for (id, tool) in [
        "list_servers",
        "list_harnesses",
        "list_secrets",
        "list_plugins",
    ]
    .into_iter()
    .enumerate()
    {
        let (frame, text) = server.call_tool(id as u64 + 1, tool, json!({}));
        assert_eq!(frame["result"]["isError"], false, "{tool}: {text}");
        assert!(
            !text.contains("sk-live-SECRETVALUE99"),
            "{tool} leaked: {text}"
        );
        assert!(
            !text.contains("another-secret-value-7"),
            "{tool} leaked: {text}"
        );
    }

    let (_, secrets) = server.call_tool(10, "list_secrets", json!({}));
    let secrets: Value = serde_json::from_str(&secrets).expect("JSON");
    assert_eq!(secrets["dotenv_variables"], json!(["OTHER_TOKEN"]));
    assert_eq!(secrets["servers_with_literal_secrets"], json!(["api"]));

    // Mutating tools stay hidden and refuse to run without MAGA_MCP_MUTATIONS=1.
    let (frame, text) = server.call_tool(
        11,
        "set_server_harness",
        json!({ "name": "api", "harness": "claude", "state": "off" }),
    );
    assert_eq!(frame["result"]["isError"], true);
    assert!(text.contains("read-only"), "{text}");
    let (frame, _) = server.call_tool(12, "install_skill", json!({ "package": "o/r" }));
    assert_eq!(frame["result"]["isError"], true);
}
