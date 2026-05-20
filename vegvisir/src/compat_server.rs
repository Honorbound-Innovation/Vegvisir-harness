use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::app::TuiApplication;

#[derive(Clone, Debug)]
pub struct CompatServerOptions {
    pub host: String,
    pub port: u16,
    pub workspace: PathBuf,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub dangerously_bypass_approvals_and_sandbox: bool,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: Option<String>,
    messages: Option<Vec<ChatMessage>>,
    stream: Option<bool>,
    conversation_id: Option<String>,
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ResponsesRequest {
    model: Option<String>,
    input: Option<Value>,
    prompt: Option<Value>,
    conversation_id: Option<String>,
    previous_response_id: Option<String>,
    stream: Option<bool>,
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    content: Value,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    message: String,
    #[serde(rename = "type")]
    kind: &'static str,
    code: Option<&'static str>,
}

type Sessions = Arc<Mutex<HashMap<String, TuiApplication>>>;

pub fn run_openai_compat_server(options: CompatServerOptions) -> anyhow::Result<()> {
    let address = format!("{}:{}", options.host, options.port);
    let server = Server::http(&address).map_err(|error| {
        anyhow::anyhow!("failed to bind compatibility server {address}: {error}")
    })?;
    eprintln!("Vegvisir OpenAI-compatible bridge listening on http://{address}");
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    for request in server.incoming_requests() {
        let options = options.clone();
        let sessions = Arc::clone(&sessions);
        if let Err(error) = handle_request(request, sessions, options) {
            eprintln!("compat request failed: {error}");
        }
    }
    Ok(())
}

fn handle_request(
    mut request: tiny_http::Request,
    sessions: Sessions,
    options: CompatServerOptions,
) -> anyhow::Result<()> {
    let path = request
        .url()
        .split('?')
        .next()
        .unwrap_or(request.url())
        .to_string();
    match (request.method(), path.as_str()) {
        (&Method::Options, _) => {
            respond_empty(request, StatusCode(204))?;
        }
        (&Method::Get, "/v1")
        | (&Method::Get, "/v1/")
        | (&Method::Head, "/v1")
        | (&Method::Head, "/v1/")
        | (&Method::Get, "/")
        | (&Method::Head, "/") => {
            respond_json(
                request,
                StatusCode(200),
                json!({
                    "name": "Vegvisir OpenAI-compatible bridge",
                    "status": "ok",
                    "endpoints": [
                        "/v1/models",
                        "/v1/chat/completions",
                        "/v1/responses"
                    ]
                }),
            )?;
        }
        (&Method::Get, "/v1/models") | (&Method::Get, "/models") => {
            let app = new_app(&options)?;
            let data = model_list(&app);
            respond_json(
                request,
                StatusCode(200),
                json!({ "object": "list", "data": data }),
            )?;
        }
        (&Method::Post, "/v1/chat/completions") | (&Method::Post, "/chat/completions") => {
            let body = read_body(&mut request)?;
            let input: ChatCompletionRequest = serde_json::from_str(&body)?;
            let session_key =
                session_key(input.conversation_id.as_deref(), input.metadata.as_ref());
            let prompt = chat_prompt(&input)?;
            let answer = match with_session(
                &sessions,
                &options,
                session_key,
                input.model.as_deref(),
                |app| app.send_headless(&prompt),
            ) {
                Ok(answer) => answer,
                Err(error) => {
                    return respond_model_error(request, input.stream.unwrap_or(false), error);
                }
            };
            if input.stream.unwrap_or(false) {
                respond_sse(
                    request,
                    vec![
                        json!({
                            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                            "object": "chat.completion.chunk",
                            "choices": [{
                                "index": 0,
                                "delta": { "content": answer },
                                "finish_reason": null
                            }]
                        }),
                        json!({
                            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                            "object": "chat.completion.chunk",
                            "choices": [{
                                "index": 0,
                                "delta": {},
                                "finish_reason": "stop"
                            }]
                        }),
                    ],
                )?;
            } else {
                respond_json(
                    request,
                    StatusCode(200),
                    json!({
                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                        "object": "chat.completion",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": answer },
                            "finish_reason": "stop"
                        }],
                        "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
                    }),
                )?;
            }
        }
        (&Method::Post, "/v1/responses") | (&Method::Post, "/responses") => {
            let body = read_body(&mut request)?;
            let input: ResponsesRequest = serde_json::from_str(&body)?;
            let session_key = session_key(
                input
                    .conversation_id
                    .as_deref()
                    .or(input.previous_response_id.as_deref()),
                input.metadata.as_ref(),
            );
            let prompt = responses_prompt(&input)?;
            let answer = match with_session(
                &sessions,
                &options,
                session_key,
                input.model.as_deref(),
                |app| app.send_headless(&prompt),
            ) {
                Ok(answer) => answer,
                Err(error) => {
                    return respond_model_error(request, input.stream.unwrap_or(false), error);
                }
            };
            let id = format!("resp_{}", uuid::Uuid::new_v4());
            if input.stream.unwrap_or(false) {
                respond_sse(
                    request,
                    vec![
                        json!({
                            "type": "response.output_text.delta",
                            "response_id": id,
                            "delta": answer,
                        }),
                        json!({
                            "type": "response.completed",
                            "response": response_body(&id, &answer),
                        }),
                    ],
                )?;
            } else {
                respond_json(request, StatusCode(200), response_body(&id, &answer))?;
            }
        }
        _ => respond_json(
            request,
            StatusCode(404),
            json!(ErrorBody {
                error: ErrorDetail {
                    message: format!("unsupported compatibility endpoint: {path}"),
                    kind: "invalid_request_error",
                    code: Some("not_found"),
                },
            }),
        )?,
    }
    Ok(())
}

fn model_list(app: &TuiApplication) -> Vec<Value> {
    let providers = app
        .provider_registry
        .list()
        .into_iter()
        .filter(|provider| provider.enabled)
        .collect::<Vec<_>>();
    let mut data = Vec::new();
    for model in app.models.list() {
        for provider in &providers {
            if app
                .models
                .is_model_allowed_for_provider(model, &provider.name)
            {
                data.push(json!({
                    "id": format!("{}/{}", provider.name, model.name),
                    "object": "model",
                    "created": 0,
                    "owned_by": provider.name,
                }));
            }
        }
    }
    data
}

fn new_app(options: &CompatServerOptions) -> anyhow::Result<TuiApplication> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        options.workspace.clone(),
        options.dangerously_bypass_approvals_and_sandbox,
    )?;
    if let Some(provider) = &options.provider {
        app.session.current_provider = provider.clone();
    }
    if let Some(model) = &options.model {
        app.session.current_model = model.clone();
    }
    if let Some(agent) = &options.agent {
        let _ = app.execute_command(&format!("/agent use {agent}"))?;
    }
    Ok(app)
}

fn with_session<F>(
    sessions: &Sessions,
    options: &CompatServerOptions,
    key: String,
    model: Option<&str>,
    f: F,
) -> anyhow::Result<String>
where
    F: FnOnce(&mut TuiApplication) -> anyhow::Result<String>,
{
    let mut sessions = sessions
        .lock()
        .map_err(|_| anyhow::anyhow!("compat session lock poisoned"))?;
    if !sessions.contains_key(&key) {
        sessions.insert(key.clone(), new_app(options)?);
    }
    let app = sessions
        .get_mut(&key)
        .ok_or_else(|| anyhow::anyhow!("compat session unavailable"))?;
    if let Some(model) = model {
        apply_requested_model(app, model)?;
    }
    f(app)
}

fn apply_requested_model(app: &mut TuiApplication, value: &str) -> anyhow::Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if let Some((provider, model)) = provider_qualified_model(value) {
        let Some(model_info) = app.models.get(&model) else {
            anyhow::bail!("Unknown Vegvisir model requested by compatibility client: {model}");
        };
        if !app
            .models
            .is_model_allowed_for_provider(model_info, &provider)
        {
            anyhow::bail!("Model {model} is not compatible with Vegvisir provider {provider}");
        }
        if app.provider_registry.get(&provider).is_none() {
            anyhow::bail!(
                "Unknown Vegvisir provider requested by compatibility client: {provider}"
            );
        }
        app.session.current_provider = provider;
        app.session.current_model = model;
        if let Some(context_window) = model_info.context_window {
            app.session.context_limit = context_window;
        }
        return Ok(());
    }
    let Some(model_info) = app.models.get(value) else {
        anyhow::bail!("Unknown Vegvisir model requested by compatibility client: {value}");
    };
    if app
        .models
        .is_model_allowed_for_provider(model_info, &app.session.current_provider)
    {
        app.session.current_model = model_info.name.clone();
        if let Some(context_window) = model_info.context_window {
            app.session.context_limit = context_window;
        }
        return Ok(());
    }
    let Some(provider) = app
        .provider_registry
        .list()
        .into_iter()
        .find(|provider| {
            provider.enabled
                && app
                    .models
                    .is_model_allowed_for_provider(model_info, &provider.name)
        })
        .map(|provider| provider.name.clone())
    else {
        anyhow::bail!(
            "No enabled Vegvisir provider can serve requested model {}",
            model_info.name
        );
    };
    app.session.current_provider = provider;
    app.session.current_model = model_info.name.clone();
    if let Some(context_window) = model_info.context_window {
        app.session.context_limit = context_window;
    }
    Ok(())
}

fn provider_qualified_model(value: &str) -> Option<(String, String)> {
    let (provider, model) = value.split_once('/')?;
    if provider.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    Some((provider.to_string(), model.to_string()))
}

fn session_key(conversation_id: Option<&str>, metadata: Option<&Value>) -> String {
    conversation_id
        .map(str::to_string)
        .or_else(|| {
            metadata
                .and_then(|value| value.get("thread_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "default".to_string())
}

fn chat_prompt(input: &ChatCompletionRequest) -> anyhow::Result<String> {
    let messages = input
        .messages
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("chat completions request requires messages"))?;
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| content_text(&message.content))
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("chat completions request requires a user message"))
}

fn responses_prompt(input: &ResponsesRequest) -> anyhow::Result<String> {
    let value = input
        .input
        .as_ref()
        .or(input.prompt.as_ref())
        .ok_or_else(|| anyhow::anyhow!("responses request requires input or prompt"))?;
    content_text(value)
}

fn content_text(value: &Value) -> anyhow::Result<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    parts.push(text.to_string());
                } else if let Some(text) = item.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                } else if let Some(text) = item.get("content").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
            Ok(parts.join("\n"))
        }
        other => Ok(other.to_string()),
    }
}

fn response_body(id: &str, answer: &str) -> Value {
    json!({
        "id": id,
        "object": "response",
        "status": "completed",
        "output_text": answer,
        "output": [{
            "id": format!("msg_{}", uuid::Uuid::new_v4()),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": answer }]
        }],
        "usage": { "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }
    })
}

fn read_body(request: &mut tiny_http::Request) -> anyhow::Result<String> {
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body)?;
    Ok(body)
}

fn respond_json(
    request: tiny_http::Request,
    status: StatusCode,
    body: Value,
) -> anyhow::Result<()> {
    let response = Response::from_string(serde_json::to_string(&body)?)
        .with_status_code(status)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
        .with_header(cors_origin_header())
        .with_header(cors_methods_header())
        .with_header(cors_headers_header());
    request.respond(response)?;
    Ok(())
}

fn respond_sse(request: tiny_http::Request, events: Vec<Value>) -> anyhow::Result<()> {
    let mut body = String::new();
    for event in events {
        body.push_str("data: ");
        body.push_str(&serde_json::to_string(&event)?);
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    let response = Response::from_string(body)
        .with_status_code(StatusCode(200))
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/event-stream"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]).unwrap())
        .with_header(cors_origin_header())
        .with_header(cors_methods_header())
        .with_header(cors_headers_header());
    request.respond(response)?;
    Ok(())
}

fn respond_model_error(
    request: tiny_http::Request,
    stream: bool,
    error: anyhow::Error,
) -> anyhow::Result<()> {
    let message = error.to_string();
    if stream {
        return respond_sse(
            request,
            vec![json!({
                "error": {
                    "message": message,
                    "type": "provider_error",
                    "code": "provider_error",
                }
            })],
        );
    }
    respond_json(
        request,
        StatusCode(502),
        json!(ErrorBody {
            error: ErrorDetail {
                message,
                kind: "provider_error",
                code: Some("provider_error"),
            },
        }),
    )
}

fn respond_empty(request: tiny_http::Request, status: StatusCode) -> anyhow::Result<()> {
    let response = Response::empty(status)
        .with_header(cors_origin_header())
        .with_header(cors_methods_header())
        .with_header(cors_headers_header());
    request.respond(response)?;
    Ok(())
}

fn cors_origin_header() -> Header {
    Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap()
}

fn cors_methods_header() -> Header {
    Header::from_bytes(
        &b"Access-Control-Allow-Methods"[..],
        &b"GET, POST, HEAD, OPTIONS"[..],
    )
    .unwrap()
}

fn cors_headers_header() -> Header {
    Header::from_bytes(
        &b"Access-Control-Allow-Headers"[..],
        &b"authorization, content-type, openai-organization, openai-project"[..],
    )
    .unwrap()
}
