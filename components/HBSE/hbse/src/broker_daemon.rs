use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use ureq::Agent;
use uuid::Uuid;

use crate::policy::{AccessRequest, DeliveryMode};
use crate::provider::PASSPHRASE_PROVIDER_ID;
use crate::provider_system::SYSTEM_FINGERPRINT_PROVIDER_ID;
use crate::provider_tpm2::TPM2_PROVIDER_ID;
use crate::provider_tpm2_esapi::TPM2_ESAPI_PROVIDER_ID;
use crate::store::SQLiteVaultStore;
use crate::vault::LocalVault;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Vault(#[from] crate::vault::VaultError),
    #[error("broker is locked")]
    Locked,
    #[error("unsupported broker command: {0}")]
    UnknownCommand(String),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error("brokered HTTP URL must use http or https and include a host")]
    InvalidHttpUrl,
    #[error("request contains HBSE-managed secret outside credential injection")]
    RequestContainsSecret,
    #[error("brokered HTTP response body too large")]
    ResponseTooLarge,
    #[error("http error: {0}")]
    Http(String),
    #[error("HTTP gateway remote listen address requires explicit allow_remote")]
    RemoteHttpGatewayNotAllowed,
    #[error("brokered HTTP request body too large")]
    RequestBodyTooLarge,
}

#[derive(Debug, Clone)]
pub struct BrokerState {
    pub vault_path: PathBuf,
    pub idle_timeout_seconds: f64,
    unlocked_passphrase: Option<String>,
    unlocked: bool,
    mfa_verified: bool,
    broker_session_id: Option<String>,
    last_activity: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpGatewayConfig {
    pub listen: String,
    pub upstream_base_url: String,
    pub secret_ref: String,
    pub consumer: String,
    pub purpose: String,
    pub model_discovery_purpose: String,
    pub credential_header: String,
    pub credential_prefix: String,
    pub timeout_seconds: f64,
    pub max_response_bytes: u64,
    pub max_request_body_bytes: u64,
    pub allow_remote: bool,
}

impl BrokerState {
    pub fn new(vault_path: impl AsRef<Path>, idle_timeout_seconds: f64) -> Self {
        Self {
            vault_path: vault_path.as_ref().to_path_buf(),
            idle_timeout_seconds,
            unlocked_passphrase: None,
            unlocked: false,
            mfa_verified: false,
            broker_session_id: None,
            last_activity: None,
        }
    }

    fn vault(&self) -> LocalVault {
        LocalVault::new(SQLiteVaultStore::new(&self.vault_path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerIdentity {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
    pub exe_path: Option<String>,
    pub comm: Option<String>,
    pub exe_sha256: Option<String>,
}

pub fn serve(
    vault_path: impl AsRef<Path>,
    socket_path: impl AsRef<Path>,
    idle_timeout_seconds: f64,
) -> Result<(), BrokerError> {
    serve_with_http_gateway(vault_path, socket_path, idle_timeout_seconds, None)
}

pub fn serve_with_http_gateway(
    vault_path: impl AsRef<Path>,
    socket_path: impl AsRef<Path>,
    idle_timeout_seconds: f64,
    http_gateway: Option<HttpGatewayConfig>,
) -> Result<(), BrokerError> {
    let socket_path = socket_path.as_ref();
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))?;
    let state = Arc::new(Mutex::new(BrokerState::new(
        vault_path,
        idle_timeout_seconds,
    )));
    if let Some(config) = http_gateway {
        validate_http_gateway_listen(&config)?;
        let http_listener = TcpListener::bind(&config.listen)?;
        let http_state = Arc::clone(&state);
        thread::spawn(move || {
            if let Err(err) = serve_http_gateway(http_listener, http_state, config) {
                eprintln!("HBSE HTTP gateway stopped: {err}");
            }
        });
    }
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut state = state
            .lock()
            .map_err(|_| BrokerError::Http("broker state lock poisoned".to_string()))?;
        let response = match handle_connection(&mut stream, &mut state) {
            Ok(value) => value,
            Err(err) => json!({
                "ok": false,
                "error": {
                    "code": error_code(&err),
                    "message": err.to_string(),
                }
            }),
        };
        writeln!(
            stream,
            "{}",
            serde_json::to_string(&canonical_response(response))?
        )?;
    }
    Ok(())
}

pub fn request(socket_path: impl AsRef<Path>, payload: &Value) -> Result<Value, BrokerError> {
    let mut stream = UnixStream::connect(socket_path)?;
    writeln!(stream, "{}", serde_json::to_string(payload)?)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}

fn handle_connection(
    stream: &mut UnixStream,
    state: &mut BrokerState,
) -> Result<Value, BrokerError> {
    expire_idle_unlock(state);
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let request: Value = serde_json::from_str(line.trim_end())?;
    let peer = peer_identity(stream)?;
    let command = str_field(&request, "command")?;
    match command {
        "status" => Ok(json!({
            "ok": true,
            "unlocked": state.unlocked,
            "mfa_verified": state.mfa_verified,
            "idle_timeout_seconds": state.idle_timeout_seconds,
            "last_activity": state.last_activity.and_then(system_time_millis),
        })),
        "unlock" => {
            validate_unlock(state, request.get("passphrase").and_then(Value::as_str))?;
            state.mfa_verified = false;
            state.unlocked_passphrase = request
                .get("passphrase")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            if let Some(code) = request.get("mfa_code").and_then(Value::as_str) {
                if let Err(err) = validate_mfa(state, code) {
                    state.unlocked_passphrase = None;
                    return Err(err);
                }
                state.mfa_verified = true;
            }
            state.unlocked = true;
            state.broker_session_id = Some(Uuid::new_v4().to_string());
            mark_activity(state);
            Ok(json!({"ok": true, "unlocked": true, "mfa_verified": state.mfa_verified}))
        }
        "mfa_verify" => {
            require_unlocked(state)?;
            validate_mfa(state, str_field(&request, "mfa_code")?)?;
            state.mfa_verified = true;
            mark_activity(state);
            Ok(json!({"ok": true, "unlocked": true, "mfa_verified": true}))
        }
        "lock" => {
            state.unlocked_passphrase = None;
            state.unlocked = false;
            state.mfa_verified = false;
            state.broker_session_id = None;
            state.last_activity = None;
            Ok(json!({"ok": true, "unlocked": false}))
        }
        "checkout" => {
            require_unlocked(state)?;
            let vault = state.vault();
            let access = access_request(
                &request,
                &peer,
                false,
                state.mfa_verified,
                state.broker_session_id.clone(),
            )?;
            let ticket =
                vault.issue_ticket(access, state.unlocked_passphrase.as_deref().unwrap_or(""))?;
            mark_activity(state);
            Ok(json!({"ok": true, "ticket": ticket, "peer": peer}))
        }
        "materialize" => {
            require_unlocked(state)?;
            let vault = state.vault();
            let access = access_request(
                &request,
                &peer,
                request
                    .get("raw_export_requested")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                state.mfa_verified,
                state.broker_session_id.clone(),
            )?;
            let ticket = vault.issue_ticket(
                access.clone(),
                state.unlocked_passphrase.as_deref().unwrap_or(""),
            )?;
            let secret = vault.consume_ticket_for_secret(
                &ticket.ticket_id,
                access,
                state.unlocked_passphrase.as_deref().unwrap_or(""),
            )?;
            mark_activity(state);
            Ok(json!({
                "ok": true,
                "secret": String::from_utf8_lossy(&secret),
                "peer": peer,
            }))
        }
        "provider_http" => {
            require_unlocked(state)?;
            let vault = state.vault();
            let response = brokered_http_request(
                &vault,
                state.unlocked_passphrase.as_deref().unwrap_or(""),
                &request,
                &peer,
                state.mfa_verified,
                state.broker_session_id.clone(),
            )?;
            mark_activity(state);
            Ok(json!({
                "ok": true,
                "status_code": response.status_code,
                "headers": response.headers,
                "body": response.body,
                "body_base64": response.body_base64,
                "redacted": response.redacted,
                "peer": peer,
            }))
        }
        other => Err(BrokerError::UnknownCommand(other.to_string())),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrokeredHttpResponse {
    status_code: u16,
    headers: serde_json::Map<String, Value>,
    body: String,
    body_base64: Option<String>,
    redacted: bool,
}

fn brokered_http_request(
    vault: &LocalVault,
    passphrase: &str,
    request: &Value,
    peer: &PeerIdentity,
    mfa_verified: bool,
    broker_session_id: Option<String>,
) -> Result<BrokeredHttpResponse, BrokerError> {
    let url = str_field(request, "url")?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_ascii_uppercase();
    let parsed = parse_http_url(url)?;
    let body_storage;
    let body = if let Some(encoded) = request.get("body_base64").and_then(Value::as_str) {
        body_storage = base64_decode(encoded)?;
        Some(body_storage.as_slice())
    } else {
        request
            .get("body")
            .and_then(Value::as_str)
            .map(str::as_bytes)
    };
    let body_len = body.map_or(0, |value| value.len()) as u64;
    let consumer = request
        .get("consumer")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("uid:{}", peer.uid));
    let purpose = str_field(request, "purpose")?.to_string();
    let access = AccessRequest {
        secret_ref: str_field(request, "secret_ref")?.to_string(),
        consumer,
        purpose,
        delivery_mode: DeliveryMode::BrokeredHttp,
        provider_assurance: request
            .get("provider_assurance")
            .and_then(Value::as_str)
            .unwrap_or("A1")
            .to_string(),
        raw_export_requested: false,
        http_host: Some(parsed.host.clone()),
        http_scheme: Some(parsed.scheme),
        http_method: Some(method.clone()),
        http_path: Some(parsed.path),
        http_request_body_bytes: Some(body_len),
        os_uid: Some(peer.uid),
        executable_path: peer.exe_path.clone(),
        executable_sha256: peer.exe_sha256.clone(),
        mfa_verified,
        broker_session_id,
        now: chrono::Utc::now(),
    };
    let ticket = vault.issue_ticket(access.clone(), passphrase)?;
    let secret = vault.consume_ticket_for_secret(&ticket.ticket_id, access, passphrase)?;

    let mut headers = request
        .get("headers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_no_secret_leak(&secret, url, &headers, body)?;
    let credential_header = request
        .get("credential_header")
        .and_then(Value::as_str)
        .unwrap_or("Authorization")
        .to_string();
    let credential_prefix = request
        .get("credential_prefix")
        .and_then(Value::as_str)
        .unwrap_or("Bearer ");
    let credential =
        if let Some(field) = request.get("credential_json_field").and_then(Value::as_str) {
            let secret_json: Value = serde_json::from_slice(&secret)?;
            json_string_path(&secret_json, field)?
        } else {
            String::from_utf8_lossy(&secret).to_string()
        };
    headers.insert(
        credential_header,
        Value::String(format!("{}{}", credential_prefix, credential)),
    );
    if let Some(extra_headers) = request
        .get("credential_json_headers")
        .and_then(Value::as_object)
    {
        let secret_json: Value = serde_json::from_slice(&secret)?;
        for (header, field) in extra_headers {
            let Some(field) = field.as_str() else {
                continue;
            };
            headers.insert(
                header.clone(),
                Value::String(json_string_path(&secret_json, field)?),
            );
        }
    }

    let timeout = request
        .get("timeout_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let max_response_bytes = request
        .get("max_response_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(10 * 1024 * 1024) as usize;
    let response_body_base64 = request
        .get("response_body_base64")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let agent = Agent::new();
    let mut req = agent.request(&method, url);
    if timeout > 0.0 {
        req = req.timeout(Duration::from_secs_f64(timeout));
    }
    for (key, value) in &headers {
        if let Some(value) = value.as_str() {
            req = req.set(key, value);
        }
    }
    let result = match body {
        Some(body) if is_multipart_request(&headers) => send_multipart_bytes(req, body),
        Some(body) => req.send_bytes(body),
        None => req.call(),
    };
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(err) => return Err(BrokerError::Http(err.to_string())),
    };
    let status_code = response.status();
    let mut response_headers = serde_json::Map::new();
    let mut headers_changed = false;
    for key in response.headers_names() {
        if let Some(value) = response.header(&key) {
            let (redacted, changed) = redact_known_secret(value, &secret);
            headers_changed |= changed;
            response_headers.insert(key, Value::String(redacted));
        }
    }
    let mut reader = response.into_reader();
    let mut body_bytes = Vec::new();
    let limit = (max_response_bytes + 1) as u64;
    reader.by_ref().take(limit).read_to_end(&mut body_bytes)?;
    if body_bytes.len() > max_response_bytes {
        return Err(BrokerError::ResponseTooLarge);
    }
    if response_body_base64 {
        Ok(BrokeredHttpResponse {
            status_code,
            headers: response_headers,
            body: String::new(),
            body_base64: Some(base64_encode(&body_bytes)),
            redacted: headers_changed,
        })
    } else {
        let body_text = String::from_utf8_lossy(&body_bytes);
        let (body, body_changed) = redact_known_secret(&body_text, &secret);
        Ok(BrokeredHttpResponse {
            status_code,
            headers: response_headers,
            body,
            body_base64: None,
            redacted: headers_changed || body_changed,
        })
    }
}

fn validate_http_gateway_listen(config: &HttpGatewayConfig) -> Result<(), BrokerError> {
    if config.allow_remote {
        return Ok(());
    }
    let addr = config
        .listen
        .parse::<std::net::SocketAddr>()
        .map_err(|err| BrokerError::Http(format!("invalid HTTP gateway listen address: {err}")))?;
    match addr.ip() {
        IpAddr::V4(ip) if ip.is_loopback() => Ok(()),
        IpAddr::V6(ip) if ip.is_loopback() => Ok(()),
        _ => Err(BrokerError::RemoteHttpGatewayNotAllowed),
    }
}

fn serve_http_gateway(
    listener: TcpListener,
    state: Arc<Mutex<BrokerState>>,
    config: HttpGatewayConfig,
) -> Result<(), BrokerError> {
    for stream in listener.incoming() {
        let mut stream = stream?;
        let response = match handle_http_gateway_connection(&mut stream, &state, &config) {
            Ok(response) => response,
            Err(err) => HttpGatewayResponse {
                status_code: match err {
                    BrokerError::Locked => 423,
                    BrokerError::Vault(_) => 403,
                    BrokerError::RequestContainsSecret => 400,
                    BrokerError::RequestBodyTooLarge => 413,
                    BrokerError::ResponseTooLarge => 502,
                    _ => 500,
                },
                headers: vec![("Content-Type".to_string(), "application/json".to_string())],
                body: serde_json::to_vec(&json!({
                    "error": {
                        "message": err.to_string(),
                        "type": "hbse_gateway_error",
                    }
                }))?,
            },
        };
        write_http_gateway_response(&mut stream, response)?;
    }
    Ok(())
}

fn handle_http_gateway_connection(
    stream: &mut TcpStream,
    state: &Arc<Mutex<BrokerState>>,
    config: &HttpGatewayConfig,
) -> Result<HttpGatewayResponse, BrokerError> {
    let request = read_http_gateway_request(stream, config.max_request_body_bytes as usize)?;
    if request.method == "GET" && request.path == "/health" {
        return Ok(HttpGatewayResponse {
            status_code: 200,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: serde_json::to_vec(&json!({"ok": true}))?,
        });
    }
    if !request.path.starts_with("/v1/") {
        return Ok(HttpGatewayResponse {
            status_code: 404,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: serde_json::to_vec(&json!({
                "error": {
                    "message": "HBSE HTTP gateway only proxies /v1/* paths",
                    "type": "not_found",
                }
            }))?,
        });
    }
    let peer = http_gateway_peer_identity();
    let vault;
    let passphrase;
    let mfa_verified;
    let broker_session_id;
    {
        let mut state = state
            .lock()
            .map_err(|_| BrokerError::Http("broker state lock poisoned".to_string()))?;
        require_unlocked(&mut state)?;
        vault = state.vault();
        passphrase = state.unlocked_passphrase.clone().unwrap_or_default();
        mfa_verified = state.mfa_verified;
        broker_session_id = state.broker_session_id.clone();
    }
    let upstream_url = http_gateway_upstream_url(&config.upstream_base_url, &request.path);
    let purpose = if request.path.starts_with("/v1/models") {
        &config.model_discovery_purpose
    } else {
        &config.purpose
    };
    let broker_request = json!({
        "command": "provider_http",
        "secret_ref": config.secret_ref,
        "consumer": config.consumer,
        "purpose": purpose,
        "method": request.method,
        "url": upstream_url,
        "headers": request.forward_headers,
        "body": request.body,
        "credential_header": config.credential_header,
        "credential_prefix": config.credential_prefix,
        "timeout_seconds": config.timeout_seconds,
        "max_response_bytes": config.max_response_bytes,
    });
    let response = brokered_http_request(
        &vault,
        &passphrase,
        &broker_request,
        &peer,
        mfa_verified,
        broker_session_id,
    )?;
    {
        let mut state = state
            .lock()
            .map_err(|_| BrokerError::Http("broker state lock poisoned".to_string()))?;
        mark_activity(&mut state);
    }
    let headers = response
        .headers
        .into_iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key, value.to_string())))
        .filter(|(key, _)| {
            !matches!(
                key.to_ascii_lowercase().as_str(),
                "content-length" | "transfer-encoding" | "connection"
            )
        })
        .collect::<Vec<_>>();
    Ok(HttpGatewayResponse {
        status_code: response.status_code,
        headers,
        body: response.body.into_bytes(),
    })
}

struct HttpGatewayRequest {
    method: String,
    path: String,
    forward_headers: serde_json::Map<String, Value>,
    body: Option<String>,
}

struct HttpGatewayResponse {
    status_code: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_http_gateway_request(
    stream: &TcpStream,
    max_body_bytes: usize,
) -> Result<HttpGatewayRequest, BrokerError> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or(BrokerError::MissingField("http_method"))?
        .to_ascii_uppercase();
    let path = parts
        .next()
        .ok_or(BrokerError::MissingField("http_path"))?
        .to_string();
    let mut headers = serde_json::Map::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let Some((name, value)) = trimmed.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().unwrap_or(0);
            if content_length > max_body_bytes {
                return Err(BrokerError::RequestBodyTooLarge);
            }
            continue;
        }
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "authorization" | "host" | "connection" | "transfer-encoding"
        ) {
            continue;
        }
        headers.insert(name.to_string(), Value::String(value.to_string()));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(HttpGatewayRequest {
        method,
        path,
        forward_headers: headers,
        body: if body.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&body).to_string())
        },
    })
}

fn write_http_gateway_response(
    stream: &mut TcpStream,
    response: HttpGatewayResponse,
) -> Result<(), BrokerError> {
    let reason = http_reason(response.status_code);
    write!(stream, "HTTP/1.1 {} {}\r\n", response.status_code, reason)?;
    let has_content_type = response
        .headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("content-type"));
    for (key, value) in response.headers {
        write!(stream, "{key}: {value}\r\n")?;
    }
    if !has_content_type {
        write!(stream, "Content-Type: application/json\r\n")?;
    }
    write!(
        stream,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.body.len()
    )?;
    stream.write_all(&response.body)?;
    Ok(())
}

fn http_reason(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        423 => "Locked",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "OK",
    }
}

fn http_gateway_upstream_url(base_url: &str, request_path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") && request_path.starts_with("/v1/") {
        format!("{}{}", base, request_path.trim_start_matches("/v1"))
    } else {
        format!("{base}{request_path}")
    }
}

fn http_gateway_peer_identity() -> PeerIdentity {
    PeerIdentity {
        pid: std::process::id() as i32,
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
        exe_path: std::env::current_exe()
            .ok()
            .and_then(|path| path.to_str().map(ToString::to_string)),
        comm: Some("hbse-http-gateway".to_string()),
        exe_sha256: std::env::current_exe()
            .ok()
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| format!("{:x}", Sha256::digest(bytes))),
    }
}

fn validate_unlock(state: &BrokerState, passphrase: Option<&str>) -> Result<(), BrokerError> {
    let vault = state.vault();
    let header = vault.status()?;
    let provider_id = header
        .provider_binding
        .get("provider_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if provider_id == PASSPHRASE_PROVIDER_ID {
        vault.verify_audit(passphrase.unwrap_or(""))?;
        return Ok(());
    }
    if provider_id == TPM2_PROVIDER_ID || provider_id == TPM2_ESAPI_PROVIDER_ID {
        vault.verify_audit("")?;
        return Ok(());
    }
    if provider_id == SYSTEM_FINGERPRINT_PROVIDER_ID {
        vault.verify_audit("")?;
        return Ok(());
    }
    Err(BrokerError::UnsupportedProvider(provider_id.to_string()))
}

fn validate_mfa(state: &BrokerState, code: &str) -> Result<(), BrokerError> {
    let vault = state.vault();
    vault.verify_totp_mfa(state.unlocked_passphrase.as_deref().unwrap_or(""), code)?;
    Ok(())
}

fn require_unlocked(state: &mut BrokerState) -> Result<(), BrokerError> {
    expire_idle_unlock(state);
    if state.unlocked {
        Ok(())
    } else {
        Err(BrokerError::Locked)
    }
}

fn mark_activity(state: &mut BrokerState) {
    state.last_activity = Some(SystemTime::now());
}

fn expire_idle_unlock(state: &mut BrokerState) {
    if !state.unlocked || state.idle_timeout_seconds <= 0.0 {
        return;
    }
    let Some(last_activity) = state.last_activity else {
        return;
    };
    if last_activity
        .elapsed()
        .is_ok_and(|elapsed| elapsed > Duration::from_secs_f64(state.idle_timeout_seconds))
    {
        state.unlocked_passphrase = None;
        state.unlocked = false;
        state.mfa_verified = false;
        state.broker_session_id = None;
        state.last_activity = None;
    }
}

fn access_request(
    value: &Value,
    peer: &PeerIdentity,
    raw_export_requested: bool,
    mfa_verified: bool,
    broker_session_id: Option<String>,
) -> Result<AccessRequest, BrokerError> {
    let delivery_mode = parse_delivery_mode(str_field(value, "delivery_mode")?)?;
    Ok(AccessRequest {
        secret_ref: str_field(value, "secret_ref")?.to_string(),
        consumer: value
            .get("consumer")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("uid:{}", peer.uid)),
        purpose: str_field(value, "purpose")?.to_string(),
        delivery_mode,
        provider_assurance: value
            .get("provider_assurance")
            .and_then(Value::as_str)
            .unwrap_or("A1")
            .to_string(),
        raw_export_requested,
        http_host: value
            .get("http_host")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        http_scheme: value
            .get("http_scheme")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        http_method: value
            .get("http_method")
            .or_else(|| value.get("method"))
            .and_then(Value::as_str)
            .map(|method| method.to_ascii_uppercase()),
        http_path: value
            .get("http_path")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        http_request_body_bytes: value.get("http_request_body_bytes").and_then(Value::as_u64),
        os_uid: Some(peer.uid),
        executable_path: peer.exe_path.clone(),
        executable_sha256: peer.exe_sha256.clone(),
        mfa_verified,
        broker_session_id,
        now: chrono::Utc::now(),
    })
}

fn parse_delivery_mode(value: &str) -> Result<DeliveryMode, BrokerError> {
    match value {
        "brokered_http" => Ok(DeliveryMode::BrokeredHttp),
        "brokered_operation" => Ok(DeliveryMode::BrokeredOperation),
        "callback" => Ok(DeliveryMode::Callback),
        "pipe" => Ok(DeliveryMode::Pipe),
        "fd" => Ok(DeliveryMode::Fd),
        "temp_file" => Ok(DeliveryMode::TempFile),
        "child_env" => Ok(DeliveryMode::ChildEnv),
        "raw" => Ok(DeliveryMode::Raw),
        "terminal_print" => Ok(DeliveryMode::TerminalPrint),
        other => Err(BrokerError::UnknownCommand(format!(
            "delivery_mode:{other}"
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedHttpUrl {
    scheme: String,
    host: String,
    path: String,
}

fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, BrokerError> {
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(BrokerError::InvalidHttpUrl);
    };
    if scheme != "http" && scheme != "https" {
        return Err(BrokerError::InvalidHttpUrl);
    }
    let (authority, path_part) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() {
        return Err(BrokerError::InvalidHttpUrl);
    }
    let host = authority.to_ascii_lowercase();
    let path = format!("/{}", path_part);
    Ok(ParsedHttpUrl {
        scheme: scheme.to_string(),
        host,
        path,
    })
}

fn assert_no_secret_leak(
    secret: &[u8],
    url: &str,
    headers: &serde_json::Map<String, Value>,
    body: Option<&[u8]>,
) -> Result<(), BrokerError> {
    let values = std::iter::once(url.to_string()).chain(
        headers
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|value| format!("{key}: {value}"))),
    );
    for value in values {
        if contains_secret(&value, secret) {
            return Err(BrokerError::RequestContainsSecret);
        }
    }
    if let Some(body) = body {
        if contains_secret(&String::from_utf8_lossy(body), secret) {
            return Err(BrokerError::RequestContainsSecret);
        }
    }
    Ok(())
}

fn is_multipart_request(headers: &serde_json::Map<String, Value>) -> bool {
    headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("content-type")
            && value
                .as_str()
                .map(|value| {
                    value
                        .to_ascii_lowercase()
                        .starts_with("multipart/form-data")
                })
                .unwrap_or(false)
    })
}

fn send_multipart_bytes(
    request: ureq::Request,
    body: &[u8],
) -> Result<ureq::Response, ureq::Error> {
    // Multipart providers are often strict about request framing. ureq::Request::send(reader)
    // uses chunked transfer encoding unless Content-Length is set, and some OpenAI-compatible
    // transcription endpoints reject chunked multipart bodies as "Could not parse multipart
    // form". Set the exact byte count before streaming the caller-built body so the boundary
    // and payload reach the provider unchanged.
    request
        .set("Content-Length", &body.len().to_string())
        .send(std::io::Cursor::new(body.to_vec()))
}

fn json_string_path(value: &Value, path: &str) -> Result<String, BrokerError> {
    let mut current = value;
    for segment in path.split('.') {
        current = current
            .get(segment)
            .ok_or_else(|| BrokerError::Http(format!("credential JSON field not found: {path}")))?;
    }
    current
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| BrokerError::Http(format!("credential JSON field is not a string: {path}")))
}

fn redact_known_secret(value: &str, secret: &[u8]) -> (String, bool) {
    let mut redacted = value.to_string();
    let mut representations = secret_representations(secret);
    representations.sort_by_key(|value| std::cmp::Reverse(value.len()));
    for representation in representations {
        redacted = redacted.replace(&representation, "[REDACTED:hbse-secret]");
    }
    let changed = redacted != value;
    (redacted, changed)
}

fn contains_secret(value: &str, secret: &[u8]) -> bool {
    secret_representations(secret)
        .iter()
        .any(|representation| value.contains(representation))
}

fn secret_representations(secret: &[u8]) -> Vec<String> {
    let mut values = vec![
        base64_encode(secret),
        crate::serialization::b64url_no_padding(secret),
    ];
    if let Ok(text) = std::str::from_utf8(secret) {
        values.push(text.to_string());
        values.push(percent_encode(text));
    }
    values.sort();
    values.dedup();
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect()
}

fn base64_decode(value: &str) -> Result<Vec<u8>, BrokerError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|err| BrokerError::Http(format!("invalid base64 request body: {err}")))
}

fn base64_encode(value: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn str_field<'a>(value: &'a Value, field: &'static str) -> Result<&'a str, BrokerError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(BrokerError::MissingField(field))
}

fn peer_identity(stream: &UnixStream) -> Result<PeerIdentity, BrokerError> {
    let fd = stream.as_raw_fd();
    let mut creds = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut creds as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(BrokerError::Io(std::io::Error::last_os_error()));
    }
    let (exe_path, comm, exe_sha256) = linux_process_identity(creds.pid);
    Ok(PeerIdentity {
        pid: creds.pid,
        uid: creds.uid,
        gid: creds.gid,
        exe_path,
        comm,
        exe_sha256,
    })
}

fn linux_process_identity(pid: i32) -> (Option<String>, Option<String>, Option<String>) {
    let proc = PathBuf::from("/proc").join(pid.to_string());
    let exe_path = fs::read_link(proc.join("exe"))
        .ok()
        .map(|path| path.to_string_lossy().to_string());
    let comm = fs::read_to_string(proc.join("comm"))
        .ok()
        .map(|value| value.trim().to_string());
    let exe_sha256 = exe_path
        .as_ref()
        .and_then(|path| fs::read(path).ok())
        .map(|bytes| hex_sha256(&bytes));
    (exe_path, comm, exe_sha256)
}

fn hex_sha256(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn system_time_millis(value: SystemTime) -> Option<String> {
    let millis = value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_millis();
    Some(millis.to_string())
}

fn canonical_response(value: Value) -> Value {
    value
}

fn error_code(err: &BrokerError) -> &'static str {
    match err {
        BrokerError::Locked => "LOCKED",
        BrokerError::UnknownCommand(_) => "UNKNOWN_COMMAND",
        BrokerError::MissingField(_) => "BAD_REQUEST",
        BrokerError::UnsupportedProvider(_) => "UNSUPPORTED_PROVIDER",
        BrokerError::InvalidHttpUrl => "BAD_REQUEST",
        BrokerError::RequestContainsSecret => "SECRET_LEAK_BLOCKED",
        BrokerError::ResponseTooLarge => "RESPONSE_TOO_LARGE",
        BrokerError::Http(_) => "HTTP_ERROR",
        BrokerError::RemoteHttpGatewayNotAllowed => "REMOTE_HTTP_GATEWAY_NOT_ALLOWED",
        BrokerError::RequestBodyTooLarge => "REQUEST_BODY_TOO_LARGE",
        BrokerError::Vault(_) => "VAULT_ERROR",
        BrokerError::Io(_) => "IO_ERROR",
        BrokerError::Json(_) => "JSON_ERROR",
    }
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::delivery_mode_string;

    #[test]
    fn parse_delivery_modes_for_broker_protocol() {
        assert_eq!(
            delivery_mode_string(parse_delivery_mode("terminal_print").unwrap()),
            "terminal_print"
        );
    }

    #[test]
    fn redact_known_secret_catches_text_base64_and_url_encoded_forms() {
        let secret = b"sk test";
        assert!(contains_secret("Authorization: Bearer sk test", secret));
        assert!(contains_secret("c2sgdGVzdA==", secret));
        assert!(contains_secret("sk%20test", secret));
        let (redacted, changed) = redact_known_secret("token=sk test", secret);
        assert!(changed);
        assert_eq!(redacted, "token=[REDACTED:hbse-secret]");
    }

    #[test]
    fn http_gateway_maps_openai_compatible_v1_paths() {
        assert_eq!(
            http_gateway_upstream_url("https://api.openai.com/v1", "/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            http_gateway_upstream_url("https://api.openai.com", "/v1/models"),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn multipart_detection_is_case_insensitive() {
        let mut headers = serde_json::Map::new();
        headers.insert(
            "Content-Type".to_string(),
            Value::String("multipart/form-data; boundary=vegvisir-test".to_string()),
        );
        assert!(is_multipart_request(&headers));

        headers.insert(
            "Content-Type".to_string(),
            Value::String("application/json".to_string()),
        );
        assert!(!is_multipart_request(&headers));
    }

    #[test]
    fn http_gateway_rejects_remote_bind_by_default() {
        let config = HttpGatewayConfig {
            listen: "0.0.0.0:8080".to_string(),
            upstream_base_url: "https://api.example.test".to_string(),
            secret_ref: "secret://api".to_string(),
            consumer: "hbse.http-gateway".to_string(),
            purpose: "model.chat".to_string(),
            model_discovery_purpose: "model.discovery".to_string(),
            credential_header: "Authorization".to_string(),
            credential_prefix: "Bearer ".to_string(),
            timeout_seconds: 0.0,
            max_response_bytes: 1024,
            max_request_body_bytes: 1024,
            allow_remote: false,
        };
        assert!(matches!(
            validate_http_gateway_listen(&config),
            Err(BrokerError::RemoteHttpGatewayNotAllowed)
        ));

        let mut loopback = config.clone();
        loopback.listen = "127.0.0.1:8080".to_string();
        assert!(validate_http_gateway_listen(&loopback).is_ok());

        let mut explicitly_remote = config;
        explicitly_remote.allow_remote = true;
        assert!(validate_http_gateway_listen(&explicitly_remote).is_ok());
    }
}
