use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    os::fd::AsRawFd,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Map, Value, json};

use crate::{
    core::{McpConfigStore, McpServerConfig, McpToolConfig, McpTransport},
    policy::{RuntimeGateRequest, RuntimePolicy},
    tools::{Tool, ToolRegistry},
    types::Observation,
};

type StdioMcpSessionPool = Arc<Mutex<BTreeMap<String, StdioMcpSession>>>;
type HttpMcpSessionPool = Arc<Mutex<BTreeMap<String, String>>>;

const MCP_STDIO_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const MCP_STDIO_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct McpRuntimeState {
    stdio_pool: StdioMcpSessionPool,
    http_sessions: HttpMcpSessionPool,
}

impl McpRuntimeState {
    fn new() -> Self {
        Self {
            stdio_pool: Arc::new(Mutex::new(BTreeMap::new())),
            http_sessions: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

pub fn load_mcp_servers(data_root: impl AsRef<Path>) -> anyhow::Result<Vec<McpServerConfig>> {
    let mut servers = McpConfigStore::new(data_root.as_ref().join("mcp.json")).load()?;
    for server in &mut servers {
        if server.enabled && server.tools.is_empty() {
            match discover_mcp_tools(server) {
                Ok(tools) => server.tools = tools,
                Err(error) => server.discovery_error = Some(error.to_string()),
            }
        }
    }
    Ok(servers)
}

pub fn register_mcp_tools(
    registry: &mut ToolRegistry,
    servers: &[McpServerConfig],
    runtime_policy: RuntimePolicy,
) -> anyhow::Result<()> {
    let runtime = McpRuntimeState::new();
    for server in servers.iter().filter(|server| server.enabled) {
        validate_hbse_boundary(server)?;
        let tools = effective_tools(server);
        for tool in &tools {
            let server = server.clone();
            let tool_name = tool.name.clone();
            let policy = runtime_policy.clone();
            let runtime = runtime.clone();
            let namespaced = format!("mcp::{}::{}", server.id, tool.name);
            let description = if tool.description.is_empty() {
                format!("MCP tool {} from server {}", tool.name, server.id)
            } else {
                tool.description.clone()
            };
            let schema = if tool.schema.is_null() {
                json!({"properties": {}})
            } else {
                tool.schema.clone()
            };
            registry.register(Tool::new(
                namespaced,
                description,
                Arc::new(move |args| {
                    let decision = policy.gate(RuntimeGateRequest {
                        operation: "mcp_tool_call".to_string(),
                        target: format!("{}::{}", server.id, tool_name),
                        args_summary: Value::Object(args.clone()),
                    });
                    if !decision.allowed {
                        return Observation::err(decision.reason, "RuntimePolicyDenied");
                    }
                    match call_mcp_tool_with_runtime(&server, &tool_name, args, &runtime) {
                        Ok(observation) => observation,
                        Err(error) => Observation::err(error.to_string(), "McpTransportError"),
                    }
                }),
                schema,
                true,
            ))?;
        }
    }
    Ok(())
}

fn effective_tools(server: &McpServerConfig) -> Vec<McpToolConfig> {
    server.tools.clone()
}

pub fn discover_mcp_tools(server: &McpServerConfig) -> anyhow::Result<Vec<McpToolConfig>> {
    match server.transport {
        McpTransport::Stdio => discover_stdio_tools(server),
        McpTransport::Http => discover_http_tools(server).or_else(|_| Ok(Vec::new())),
    }
}

pub fn call_mcp_tool(
    server: &McpServerConfig,
    tool_name: &str,
    args: Map<String, Value>,
) -> anyhow::Result<Observation> {
    call_mcp_tool_with_runtime(server, tool_name, args, &McpRuntimeState::new())
}

fn call_mcp_tool_with_runtime(
    server: &McpServerConfig,
    tool_name: &str,
    args: Map<String, Value>,
    runtime: &McpRuntimeState,
) -> anyhow::Result<Observation> {
    match server.transport {
        McpTransport::Stdio => call_stdio_tool(server, tool_name, args, &runtime.stdio_pool),
        McpTransport::Http => call_http_tool(server, tool_name, args, &runtime.http_sessions),
    }
}

fn call_stdio_tool(
    server: &McpServerConfig,
    tool_name: &str,
    args: Map<String, Value>,
    stdio_pool: &StdioMcpSessionPool,
) -> anyhow::Result<Observation> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": args,
        }
    });
    let response = request_pooled_stdio(stdio_pool, server, 2, payload)?;
    if let Some(error) = response.get("error") {
        return Ok(Observation::err(
            format!("MCP tool call failed: {error}"),
            "McpToolError",
        ));
    }
    let result = response.get("result").cloned().unwrap_or_else(|| json!({}));
    let content = mcp_result_text(&result);
    let mut data = Map::new();
    data.insert("server".to_string(), json!(server.id));
    data.insert("tool".to_string(), json!(tool_name));
    data.insert("result".to_string(), result);
    Ok(Observation {
        ok: true,
        content,
        data,
        error: None,
    })
}

fn request_pooled_stdio(
    pool: &StdioMcpSessionPool,
    server: &McpServerConfig,
    id: u64,
    payload: Value,
) -> anyhow::Result<Value> {
    match request_pooled_stdio_once(pool, server, id, payload.clone()) {
        Ok(response) => Ok(response),
        Err(first_error) => {
            let key = stdio_session_key(server);
            if let Ok(mut sessions) = pool.lock() {
                sessions.remove(&key);
            }
            request_pooled_stdio_once(pool, server, id, payload).map_err(|second_error| {
                anyhow::anyhow!(
                    "MCP stdio request failed after restart: {second_error}; first failure: {first_error}"
                )
            })
        }
    }
}

fn request_pooled_stdio_once(
    pool: &StdioMcpSessionPool,
    server: &McpServerConfig,
    id: u64,
    payload: Value,
) -> anyhow::Result<Value> {
    let key = stdio_session_key(server);
    let mut sessions = pool
        .lock()
        .map_err(|_| anyhow::anyhow!("MCP stdio session pool lock poisoned"))?;
    if !sessions.contains_key(&key) {
        sessions.insert(
            key.clone(),
            StdioMcpSession::start(server, MCP_STDIO_REQUEST_TIMEOUT)?,
        );
    }
    let session = sessions
        .get_mut(&key)
        .ok_or_else(|| anyhow::anyhow!("MCP stdio session {} unavailable", server.id))?;
    session.request(id, payload, MCP_STDIO_REQUEST_TIMEOUT)
}

fn stdio_session_key(server: &McpServerConfig) -> String {
    server.id.clone()
}

fn discover_stdio_tools(server: &McpServerConfig) -> anyhow::Result<Vec<McpToolConfig>> {
    let mut session = StdioMcpSession::start(server, MCP_STDIO_DISCOVERY_TIMEOUT)?;
    let response = session.request(
        2,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
        MCP_STDIO_DISCOVERY_TIMEOUT,
    )?;
    session.shutdown();
    if let Some(error) = response.get("error") {
        anyhow::bail!("MCP tools/list failed: {error}");
    }
    Ok(parse_mcp_tools_list(&response))
}

fn call_http_tool(
    server: &McpServerConfig,
    tool_name: &str,
    args: Map<String, Value>,
    http_sessions: &HttpMcpSessionPool,
) -> anyhow::Result<Observation> {
    let response = hbse_mcp_json_rpc(
        server,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": args,
            }
        }),
        Some(http_sessions),
    )?;
    if let Some(error) = response.get("error") {
        return Ok(Observation::err(
            format!("MCP HTTP tool call failed: {error}"),
            "McpToolError",
        ));
    }
    let result = response.get("result").cloned().unwrap_or_else(|| json!({}));
    let content = mcp_result_text(&result);
    let mut data = Map::new();
    data.insert("server".to_string(), json!(server.id));
    data.insert("tool".to_string(), json!(tool_name));
    data.insert("transport".to_string(), json!(server.transport));
    data.insert("result".to_string(), result);
    Ok(Observation {
        ok: true,
        content,
        data,
        error: None,
    })
}

fn discover_http_tools(server: &McpServerConfig) -> anyhow::Result<Vec<McpToolConfig>> {
    let response = hbse_mcp_json_rpc(
        server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
        None,
    )?;
    if let Some(error) = response.get("error") {
        anyhow::bail!("MCP HTTP tools/list failed: {error}");
    }
    Ok(parse_mcp_tools_list(&response))
}

fn hbse_mcp_json_rpc(
    server: &McpServerConfig,
    rpc: Value,
    http_sessions: Option<&HttpMcpSessionPool>,
) -> anyhow::Result<Value> {
    let url = server
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("MCP HTTP server {} has no url", server.id))?;
    let secret_ref = server.hbse_secret_refs.first().ok_or_else(|| {
        anyhow::anyhow!(
            "MCP HTTP server {} requires an HBSE secret ref; plaintext auth is not allowed",
            server.id
        )
    })?;
    let headers = mcp_http_headers(server, http_sessions)?;
    let payload = json!({
        "command": "provider_http",
        "secret_ref": secret_ref,
        "consumer": if server.consumer.is_empty() { format!("vegvisir.mcp.{}", server.id) } else { server.consumer.clone() },
        "purpose": if server.purpose.is_empty() { "mcp.tool.call".to_string() } else { server.purpose.clone() },
        "method": "POST",
        "url": url,
        "headers": headers,
        "body": serde_json::to_string(&rpc)?,
        "credential_header": server.metadata.get("credential_header").and_then(Value::as_str).unwrap_or("Authorization"),
        "credential_prefix": server.metadata.get("credential_prefix").and_then(Value::as_str).unwrap_or("Bearer "),
        "timeout_seconds": server.metadata.get("timeout_seconds").and_then(Value::as_f64).unwrap_or(0.0),
        "max_response_bytes": server.metadata.get("max_response_bytes").and_then(Value::as_u64).unwrap_or(10 * 1024 * 1024),
    });
    let response = hbse_broker_request(server, payload)?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        anyhow::bail!(
            "HBSE MCP request failed: {}",
            response
                .get("error")
                .cloned()
                .unwrap_or_else(|| json!({"message": "unknown HBSE error"}))
        );
    }
    let status_code = response
        .get("status_code")
        .and_then(Value::as_u64)
        .unwrap_or(500);
    let body = response.get("body").and_then(Value::as_str).unwrap_or("");
    if !(200..300).contains(&status_code) {
        anyhow::bail!("MCP HTTP server returned status {status_code}: {body}");
    }
    if let Some(http_sessions) = http_sessions
        && let Some(session_id) = response_header(&response, "mcp-session-id")
    {
        let mut sessions = http_sessions
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP HTTP session pool lock poisoned"))?;
        sessions.insert(server.id.clone(), session_id.to_string());
    }
    Ok(serde_json::from_str(body)?)
}

fn mcp_http_headers(
    server: &McpServerConfig,
    http_sessions: Option<&HttpMcpSessionPool>,
) -> anyhow::Result<Value> {
    let mut headers = Map::new();
    headers.insert("Content-Type".to_string(), json!("application/json"));
    headers.insert("Accept".to_string(), json!("application/json"));
    headers.insert("MCP-Protocol-Version".to_string(), json!("2024-11-05"));
    if let Some(http_sessions) = http_sessions
        && let Some(session_id) = http_sessions
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP HTTP session pool lock poisoned"))?
            .get(&server.id)
            .cloned()
    {
        headers.insert("Mcp-Session-Id".to_string(), json!(session_id));
    }
    Ok(Value::Object(headers))
}

fn response_header<'a>(response: &'a Value, name: &str) -> Option<&'a str> {
    response
        .get("headers")
        .and_then(Value::as_object)?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.as_str())
        .filter(|value| !value.trim().is_empty())
}

fn hbse_broker_request(server: &McpServerConfig, payload: Value) -> anyhow::Result<Value> {
    let socket_path = hbse_socket_path(server);
    let mut stream = UnixStream::connect(&socket_path)?;
    writeln!(stream, "{}", serde_json::to_string(&payload)?)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}

fn hbse_socket_path(server: &McpServerConfig) -> PathBuf {
    if let Some(path) = server.metadata.get("hbse_socket").and_then(Value::as_str) {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("HBSE_BROKER_SOCKET") {
        return PathBuf::from(path);
    }
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("hbse").join("broker.sock");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("hbse")
        .join("broker.sock")
}

fn parse_mcp_tools_list(response: &Value) -> Vec<McpToolConfig> {
    response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?.to_string();
            Some(McpToolConfig {
                name,
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                schema: tool
                    .get("inputSchema")
                    .or_else(|| tool.get("schema"))
                    .cloned()
                    .unwrap_or_else(|| json!({"properties": {}})),
            })
        })
        .collect::<Vec<_>>()
}

struct StdioMcpSession {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl StdioMcpSession {
    fn start(server: &McpServerConfig, timeout: Duration) -> anyhow::Result<Self> {
        let command = server
            .command
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("MCP stdio server {} has no command", server.id))?;
        let mut child = Command::new(command)
            .args(&server.args)
            .current_dir(
                server
                    .working_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                    }),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("MCP stdio server {} stdin unavailable", server.id))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("MCP stdio server {} stdout unavailable", server.id))?;
        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        let initialize = session.request(
            1,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "vegvisir-rust", "version": env!("CARGO_PKG_VERSION")}
                }
            }),
            timeout,
        )?;
        if let Some(error) = initialize.get("error") {
            session.shutdown();
            anyhow::bail!("MCP initialize failed: {error}");
        }
        write_mcp_message(
            &mut session.stdin,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}),
        )?;
        Ok(session)
    }

    fn request(&mut self, id: u64, payload: Value, timeout: Duration) -> anyhow::Result<Value> {
        write_mcp_message(&mut self.stdin, &payload)?;
        read_mcp_response(&mut self.stdout, id, timeout)
    }

    fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for StdioMcpSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn write_mcp_message(writer: &mut impl Write, payload: &Value) -> anyhow::Result<()> {
    let body = serde_json::to_vec(payload)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

fn read_mcp_response(
    reader: &mut BufReader<std::process::ChildStdout>,
    id: u64,
    timeout: Duration,
) -> anyhow::Result<Value> {
    loop {
        let message = read_mcp_message(reader, timeout)?;
        if message.get("id").and_then(Value::as_u64) == Some(id) {
            return Ok(message);
        }
    }
}

fn read_mcp_message(
    reader: &mut BufReader<std::process::ChildStdout>,
    timeout: Duration,
) -> anyhow::Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        wait_for_stdio_readable(reader, timeout)?;
        if reader.read_line(&mut line)? == 0 {
            anyhow::bail!("MCP server closed stdout before sending a complete message");
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length =
        content_length.ok_or_else(|| anyhow::anyhow!("MCP message missing Content-Length"))?;
    let mut body = vec![0u8; length];
    read_exact_with_timeout(reader, &mut body, timeout)?;
    Ok(serde_json::from_slice(&body)?)
}

fn read_exact_with_timeout(
    reader: &mut BufReader<std::process::ChildStdout>,
    mut body: &mut [u8],
    timeout: Duration,
) -> anyhow::Result<()> {
    while !body.is_empty() {
        wait_for_stdio_readable(reader, timeout)?;
        let read = reader.read(body)?;
        if read == 0 {
            anyhow::bail!("MCP server closed stdout before sending a complete message");
        }
        body = &mut body[read..];
    }
    Ok(())
}

fn wait_for_stdio_readable(
    reader: &BufReader<std::process::ChildStdout>,
    timeout: Duration,
) -> anyhow::Result<()> {
    if !reader.buffer().is_empty() {
        return Ok(());
    }
    let mut fd = libc::pollfd {
        fd: reader.get_ref().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let result =
            unsafe { libc::poll(&mut fd, 1, timeout.as_millis().min(i32::MAX as u128) as i32) };
        if result > 0 {
            if fd.revents & libc::POLLIN != 0 {
                return Ok(());
            }
            if fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                anyhow::bail!("MCP server stdout closed during request");
            }
        } else if result == 0 {
            anyhow::bail!("MCP stdio request timed out after {}s", timeout.as_secs());
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
    }
}

fn mcp_result_text(result: &Value) -> String {
    let Some(content) = result.get("content").and_then(Value::as_array) else {
        return serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
    };
    let parts = content
        .iter()
        .filter_map(|item| {
            item.get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| item.get("data").map(Value::to_string))
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string())
    } else {
        parts.join("\n")
    }
}

pub fn validate_hbse_boundary(server: &McpServerConfig) -> anyhow::Result<()> {
    if !server.hbse_secret_refs.is_empty() {
        if server.consumer.trim().is_empty() {
            anyhow::bail!(
                "MCP server {} has HBSE secret refs but no consumer",
                server.id
            );
        }
        if server.purpose.trim().is_empty() {
            anyhow::bail!(
                "MCP server {} has HBSE secret refs but no purpose",
                server.id
            );
        }
    }
    for (key, value) in &server.metadata {
        if key.to_ascii_lowercase().contains("secret") && value.as_str().is_some() {
            anyhow::bail!(
                "MCP server {} metadata must not contain plaintext secret-like values; use hbse_secret_refs",
                server.id
            );
        }
    }
    Ok(())
}
