use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const CHATGPT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const ISSUER: &str = "https://auth.openai.com";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const SCOPE: &str = "openid profile email offline_access api.connectors.read api.connectors.invoke";
const DEFAULT_PORTS: [u16; 2] = [1455, 1457];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenAISsoTokens {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: String,
    pub last_refresh: f64,
}

#[derive(Clone, Debug)]
pub struct OpenAISsoAuthStore {
    pub root: PathBuf,
    pub path: PathBuf,
}

impl OpenAISsoAuthStore {
    pub fn new(root: Option<PathBuf>) -> Self {
        let root = root
            .or_else(|| std::env::var_os("VEGVISIR_HOME").map(PathBuf::from))
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".vegvisir")
            });
        let path = root.join("auth").join("openai_sso.json");
        Self { root, path }
    }

    pub fn load(&self) -> anyhow::Result<Option<OpenAISsoTokens>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let data: Value = serde_json::from_str(&fs::read_to_string(&self.path)?)?;
        Ok(Some(OpenAISsoTokens::from_store_json(&data)?))
    }

    pub fn save(&self, tokens: &OpenAISsoTokens) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(&tokens.as_json())?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }

    pub fn status(&self) -> String {
        match self.load() {
            Ok(Some(tokens)) => {
                let suffix = jwt_exp(&tokens.access_token)
                    .map(|expiry| {
                        let minutes = ((expiry - now_seconds()).max(0.0) as u64) / 60;
                        format!("; access token expires in {minutes} min")
                    })
                    .unwrap_or_default();
                format!(
                    "OpenAI SSO is logged in for account {}{}.",
                    tokens.account_id, suffix
                )
            }
            _ => "OpenAI SSO is not logged in. Run /auth openai-sso.".to_string(),
        }
    }
}

impl OpenAISsoTokens {
    pub fn from_store_json(data: &Value) -> anyhow::Result<Self> {
        let tokens = data
            .get("tokens")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                anyhow::anyhow!("OpenAI SSO auth data is incomplete. Run /auth openai-sso.")
            })?;
        let id_token = tokens
            .get("id_token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let access_token = tokens
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!("OpenAI SSO auth data is incomplete. Run /auth openai-sso.")
            })?
            .to_string();
        let refresh_token = tokens
            .get("refresh_token")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!("OpenAI SSO auth data is incomplete. Run /auth openai-sso.")
            })?
            .to_string();
        let account_id = tokens
            .get("account_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| jwt_claim(&id_token, "chatgpt_account_id"))
            .ok_or_else(|| {
                anyhow::anyhow!("OpenAI SSO auth data is incomplete. Run /auth openai-sso.")
            })?;
        Ok(Self {
            id_token,
            access_token,
            refresh_token,
            account_id,
            last_refresh: data
                .get("last_refresh")
                .and_then(Value::as_f64)
                .unwrap_or_else(now_seconds),
        })
    }

    pub fn as_json(&self) -> Value {
        serde_json::json!({
            "provider": "openai-sso",
            "issuer": ISSUER,
            "tokens": {
                "id_token": self.id_token,
                "access_token": self.access_token,
                "refresh_token": self.refresh_token,
                "account_id": self.account_id,
            },
            "last_refresh": self.last_refresh,
        })
    }
}

pub fn load_fresh_tokens() -> anyhow::Result<OpenAISsoTokens> {
    load_fresh_tokens_from_store(OpenAISsoAuthStore::new(None), ISSUER)
}

pub fn login(
    root: Option<PathBuf>,
    open_browser: bool,
    timeout: Duration,
) -> anyhow::Result<String> {
    login_with_issuer(root, open_browser, timeout, ISSUER)
}

pub fn login_with_issuer(
    root: Option<PathBuf>,
    open_browser: bool,
    timeout: Duration,
    issuer: &str,
) -> anyhow::Result<String> {
    let store = OpenAISsoAuthStore::new(root);
    let verifier = token_urlsafe();
    let challenge = code_challenge(&verifier);
    let state = token_urlsafe();
    let (listener, redirect_uri) = bind_callback_listener()?;
    let auth_url = authorize_url(issuer, &redirect_uri, &challenge, &state);
    if open_browser {
        open_url(&auth_url);
    }
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "OpenAI SSO login timed out. Open this URL manually:\n{auth_url}"
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    };
    let request = read_http_request(&mut stream)?;
    let (code, callback_state) = parse_callback_request(&request)?;
    if callback_state != state {
        write_http_response(
            &mut stream,
            400,
            "Vegvisir OpenAI SSO login failed. You can close this tab.",
        )?;
        anyhow::bail!("OpenAI SSO callback state did not match.");
    }
    let tokens = exchange_code_with_issuer(&code, &redirect_uri, &verifier, issuer)?;
    store.save(&tokens)?;
    write_http_response(
        &mut stream,
        200,
        "Vegvisir OpenAI SSO login complete. You can close this tab.",
    )?;
    Ok(format!(
        "OpenAI SSO login complete for account {}.",
        tokens.account_id
    ))
}

pub fn exchange_code(
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> anyhow::Result<OpenAISsoTokens> {
    exchange_code_with_issuer(code, redirect_uri, verifier, ISSUER)
}

pub fn exchange_code_with_issuer(
    code: &str,
    redirect_uri: &str,
    verifier: &str,
    issuer: &str,
) -> anyhow::Result<OpenAISsoTokens> {
    let url = format!("{}/oauth/token", issuer.trim_end_matches('/'));
    let data: Value = ureq::post(&url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ])
        .map_err(|error| match error {
            ureq::Error::Status(code, response) => {
                let detail = response.into_string().unwrap_or_default();
                anyhow::anyhow!(
                    "OpenAI SSO token request failed: {} {}",
                    code,
                    detail.chars().take(300).collect::<String>()
                )
            }
            other => anyhow::anyhow!("OpenAI SSO token request failed: {other}"),
        })?
        .into_json()?;
    let id_token = data
        .get("id_token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let access_token = data
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("OpenAI SSO token response omitted access_token."))?
        .to_string();
    let refresh_token = data
        .get("refresh_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("OpenAI SSO token response omitted refresh_token."))?
        .to_string();
    let account_id = jwt_claim(&id_token, "chatgpt_account_id")
        .or_else(|| jwt_claim(&access_token, "chatgpt_account_id"))
        .ok_or_else(|| anyhow::anyhow!("OpenAI SSO token did not include a ChatGPT account id."))?;
    Ok(OpenAISsoTokens {
        id_token,
        access_token,
        refresh_token,
        account_id,
        last_refresh: now_seconds(),
    })
}

pub fn load_fresh_tokens_for_metadata(
    metadata: &BTreeMap<String, Value>,
) -> anyhow::Result<OpenAISsoTokens> {
    let root = metadata
        .get("auth_root")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    load_fresh_tokens_from_store(OpenAISsoAuthStore::new(root), ISSUER)
}

pub fn load_fresh_tokens_from_store(
    store: OpenAISsoAuthStore,
    issuer: &str,
) -> anyhow::Result<OpenAISsoTokens> {
    let Some(tokens) = store.load()? else {
        anyhow::bail!("OpenAI SSO is not logged in. Run /auth openai-sso.");
    };
    if jwt_exp(&tokens.access_token)
        .map(|expiry| expiry < now_seconds() + 120.0)
        .unwrap_or(false)
    {
        refresh(tokens, &store, issuer)
    } else {
        Ok(tokens)
    }
}

pub fn refresh(
    tokens: OpenAISsoTokens,
    store: &OpenAISsoAuthStore,
    issuer: &str,
) -> anyhow::Result<OpenAISsoTokens> {
    let url = format!("{}/oauth/token", issuer.trim_end_matches('/'));
    let data: Value = ureq::post(&url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", &tokens.refresh_token),
        ])
        .map_err(|error| match error {
            ureq::Error::Status(code, response) => {
                let detail = response.into_string().unwrap_or_default();
                anyhow::anyhow!(
                    "OpenAI SSO token request failed: {} {}",
                    code,
                    detail.chars().take(300).collect::<String>()
                )
            }
            other => anyhow::anyhow!("OpenAI SSO token request failed: {other}"),
        })?
        .into_json()?;
    let refreshed = OpenAISsoTokens {
        id_token: data
            .get("id_token")
            .and_then(Value::as_str)
            .unwrap_or(&tokens.id_token)
            .to_string(),
        access_token: data
            .get("access_token")
            .and_then(Value::as_str)
            .unwrap_or(&tokens.access_token)
            .to_string(),
        refresh_token: data
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or(&tokens.refresh_token)
            .to_string(),
        account_id: tokens.account_id,
        last_refresh: now_seconds(),
    };
    store.save(&refreshed)?;
    Ok(refreshed)
}

pub fn codex_base_url(metadata: &BTreeMap<String, Value>) -> String {
    metadata
        .get("codex_base_url")
        .and_then(Value::as_str)
        .unwrap_or(CHATGPT_CODEX_BASE_URL)
        .trim_end_matches('/')
        .to_string()
}

pub fn auth_store_available() -> bool {
    OpenAISsoAuthStore::new(None)
        .load()
        .ok()
        .flatten()
        .is_some()
}

fn jwt_claim(token: &str, key: &str) -> Option<String> {
    let claims = jwt_claims(token);
    claims
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(Value::as_object)
                .and_then(|nested| nested.get(key))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn jwt_exp(token: &str) -> Option<f64> {
    jwt_claims(token).get("exp").and_then(Value::as_f64)
}

fn jwt_claims(token: &str) -> Value {
    let Some(payload) = token.split('.').nth(1) else {
        return Value::Object(Default::default());
    };
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(payload.as_bytes()) else {
        return Value::Object(Default::default());
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::Object(Default::default()))
}

fn bind_callback_listener() -> anyhow::Result<(TcpListener, String)> {
    for port in DEFAULT_PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            return Ok((listener, format!("http://localhost:{port}/auth/callback")));
        }
    }
    anyhow::bail!("Could not start local OpenAI SSO callback server on ports 1455 or 1457.")
}

fn authorize_url(issuer: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPE),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", "vegvisir"),
    ];
    let query = params
        .into_iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}/oauth/authorize?{query}", issuer.trim_end_matches('/'))
}

fn token_urlsafe() -> String {
    URL_SAFE_NO_PAD.encode(UuidBytes::new())
}

struct UuidBytes([u8; 32]);

impl UuidBytes {
    fn new() -> Self {
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        let mut bytes = [0_u8; 32];
        bytes[..16].copy_from_slice(first.as_bytes());
        bytes[16..].copy_from_slice(second.as_bytes());
        Self(bytes)
    }
}

impl AsRef<[u8]> for UuidBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let candidates = ["open"];
    #[cfg(not(target_os = "macos"))]
    let candidates = ["xdg-open", "sensible-browser"];
    for command in candidates {
        if Command::new(command).arg(url).spawn().is_ok() {
            break;
        }
    }
}

fn read_http_request(stream: &mut impl Read) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn parse_callback_request(request: &str) -> anyhow::Result<(String, String)> {
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("OpenAI SSO callback request was empty."))?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("OpenAI SSO callback request was malformed."))?;
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    if path != "/auth/callback" {
        anyhow::bail!("OpenAI SSO callback path was not /auth/callback.");
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = urlencoding::decode(key)?.into_owned();
        let value = urlencoding::decode(value)?.into_owned();
        match key.as_str() {
            "code" => code = Some(value),
            "state" => state = Some(value),
            "error" | "error_description" => error = Some(value),
            _ => {}
        }
    }
    if let Some(error) = error {
        anyhow::bail!("{error}");
    }
    Ok((
        code.ok_or_else(|| anyhow::anyhow!("OpenAI SSO callback omitted code."))?,
        state.ok_or_else(|| anyhow::anyhow!("OpenAI SSO callback omitted state."))?,
    ))
}

fn write_http_response(stream: &mut impl Write, status: u16, message: &str) -> anyhow::Result<()> {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let body = format!("<html><body><p>{message}</p></body></html>");
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )?;
    Ok(())
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn auth_store_path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("auth").join("openai_sso.json")
}
