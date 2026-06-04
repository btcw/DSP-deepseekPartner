use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Context};
use async_stream::stream;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode, Uri},
    Router,
};
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex, RwLock},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    logs::LogStore,
    models::{
        canonical_model_id, ApiSurface, AppSettings, GatewayProfile, ProfileStatus,
        ServiceStatusKind,
    },
    storage::SettingsStore,
};

const LATEST_TOOL_REASONING_KEY: &str = "__deepseek_gateway_latest_tool_reasoning__";

#[derive(Clone)]
pub struct GatewayRegistry {
    profile: Arc<RwLock<GatewayProfile>>,
    status: Arc<RwLock<RuntimeStatus>>,
    #[cfg(test)]
    reasoning_cache: Arc<Mutex<HashMap<String, String>>>,
    shutdown: CancellationToken,
    join: Arc<Mutex<Option<JoinHandle<()>>>>,
}

#[derive(Debug, Clone)]
struct RuntimeStatus {
    kind: ServiceStatusKind,
    last_error: Option<String>,
    started_at: Option<String>,
    request_count: u64,
}

impl RuntimeStatus {
    fn stopped(port: u16, id: &str) -> ProfileStatus {
        ProfileStatus {
            id: id.to_string(),
            status: ServiceStatusKind::Stopped,
            port,
            proxy_origin: format!("http://127.0.0.1:{port}"),
            last_error: None,
            started_at: None,
            request_count: 0,
        }
    }
}

#[derive(Clone)]
struct GatewayState {
    profile: Arc<RwLock<GatewayProfile>>,
    settings: SettingsStore,
    client: Client,
    logs: LogStore,
    status: Arc<RwLock<RuntimeStatus>>,
    reasoning_cache: Arc<Mutex<HashMap<String, String>>>,
    counter: Arc<AtomicU64>,
}

impl GatewayRegistry {
    pub async fn start(
        profile: GatewayProfile,
        logs: LogStore,
        settings: SettingsStore,
    ) -> anyhow::Result<Self> {
        profile.validate().map_err(anyhow::Error::msg)?;
        let addr = SocketAddr::from(([127, 0, 0, 1], profile.port));
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("port {} is not available", profile.port))?;

        let profile_id = profile.id.clone();
        let profile_arc = Arc::new(RwLock::new(profile));
        let status = Arc::new(RwLock::new(RuntimeStatus {
            kind: ServiceStatusKind::Starting,
            last_error: None,
            started_at: None,
            request_count: 0,
        }));
        let shutdown = CancellationToken::new();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .build()?;
        let reasoning_cache = Arc::new(Mutex::new(HashMap::new()));
        let state = GatewayState {
            profile: profile_arc.clone(),
            settings,
            client,
            logs: logs.clone(),
            status: status.clone(),
            reasoning_cache: reasoning_cache.clone(),
            counter: Arc::new(AtomicU64::new(0)),
        };
        let app = Router::new().fallback(proxy_handler).with_state(state);
        let shutdown_child = shutdown.clone();
        let server_status = status.clone();
        let (ready_tx, ready_rx) = oneshot::channel();

        let join = tokio::spawn(async move {
            {
                let mut status = server_status.write().await;
                status.kind = ServiceStatusKind::Running;
                status.started_at = Some(Utc::now().to_rfc3339());
            }
            let _ = logs
                .append(
                    &profile_id,
                    "info",
                    None,
                    format!("Gateway listening on http://{addr}"),
                )
                .await;
            let _ = ready_tx.send(());
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_child.cancelled().await;
                })
                .await;
            if let Err(error) = result {
                let _ = logs
                    .append(
                        &profile_id,
                        "error",
                        None,
                        format!("Gateway server error: {error}"),
                    )
                    .await;
            }
        });

        let _ = ready_rx.await;
        Ok(Self {
            profile: profile_arc,
            status,
            #[cfg(test)]
            reasoning_cache,
            shutdown,
            join: Arc::new(Mutex::new(Some(join))),
        })
    }

    pub async fn update_profile(&self, profile: GatewayProfile) {
        let mut current = self.profile.write().await;
        *current = profile;
    }

    pub async fn stop(&self) {
        {
            let mut status = self.status.write().await;
            status.kind = ServiceStatusKind::Stopping;
        }
        self.shutdown.cancel();
        if let Some(join) = self.join.lock().await.take() {
            let _ = join.await;
        }
        let mut status = self.status.write().await;
        status.kind = ServiceStatusKind::Stopped;
    }

    pub async fn status(&self) -> ProfileStatus {
        let profile = self.profile.read().await.clone();
        let status = self.status.read().await.clone();
        ProfileStatus {
            id: profile.id,
            status: status.kind,
            port: profile.port,
            proxy_origin: format!("http://127.0.0.1:{}", profile.port),
            last_error: status.last_error,
            started_at: status.started_at,
            request_count: status.request_count,
        }
    }
}

pub async fn stopped_status(profile: &GatewayProfile) -> ProfileStatus {
    RuntimeStatus::stopped(profile.port, &profile.id)
}

async fn proxy_handler(
    State(state): State<GatewayState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let request_id = Uuid::new_v4().to_string();
    let count = state.counter.fetch_add(1, Ordering::Relaxed) + 1;
    {
        let mut status = state.status.write().await;
        status.request_count = count;
    }

    match proxy_request(
        state.clone(),
        method.clone(),
        uri.clone(),
        headers,
        body,
        request_id.clone(),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let profile = state.profile.read().await.clone();
            {
                let mut status = state.status.write().await;
                status.last_error = Some(error.to_string());
            }
            let _ = state
                .logs
                .append(
                    &profile.id,
                    "error",
                    Some(&request_id),
                    format!("{method} {} failed: {error}", uri.path()),
                )
                .await;
            let body = json!({
                "error": {
                    "message": error.to_string(),
                    "type": "deepseek_gateway_error"
                }
            });
            json_response(StatusCode::BAD_GATEWAY, body)
        }
    }
}

async fn proxy_request(
    state: GatewayState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    request_id: String,
) -> anyhow::Result<Response<Body>> {
    let profile = state.profile.read().await.clone();
    state
        .logs
        .append(
            &profile.id,
            "info",
            Some(&request_id),
            format!("Received {method} {}", uri.path()),
        )
        .await?;
    let route = route_for(uri.path(), &profile)?;

    if let RouteKind::LocalModels { surface } = route.kind {
        if method != Method::GET && method != Method::HEAD {
            return Err(anyhow!(
                "models endpoint only supports GET or HEAD, got {method}"
            ));
        }
        let body = models_response(surface, &profile);
        state
            .logs
            .append(
                &profile.id,
                "info",
                Some(&request_id),
                format!("{method} {} -> local models 200", uri.path()),
            )
            .await?;
        return Ok(json_response(StatusCode::OK, body));
    }

    let mut value = if body.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_slice::<Value>(&body).context("request body must be JSON")?
    };

    let RouteKind::Upstream {
        surface,
        upstream_path,
    } = route.kind
    else {
        unreachable!("local route returned above");
    };
    normalize_request_body(
        surface.clone(),
        &profile,
        &mut value,
        &state.reasoning_cache,
    )
    .await;
    let settings = state.settings.load().await.unwrap_or_default();
    inject_settings_context(surface.clone(), &settings, &mut value);
    inject_builtin_mcp_tools(surface.clone(), &settings, &mut value);
    let target = target_url(&profile.upstream_base_url, upstream_path);
    state
        .logs
        .append(
            &profile.id,
            "info",
            Some(&request_id),
            format!("{method} {} -> {target}", uri.path()),
        )
        .await?;

    let timeout = Duration::from_secs(profile.timeout_seconds);
    let request = state
        .client
        .request(method.clone(), target.clone())
        .timeout(timeout)
        .json(&value);
    let (request, injected_fallback_key) = apply_forwarded_headers(request, &headers, &profile);
    if injected_fallback_key {
        state
            .logs
            .append(
                &profile.id,
                "info",
                Some(&request_id),
                "Injected configured fallback API key because request did not include one",
            )
            .await?;
    }

    let upstream = request.send().await?;
    let status = upstream.status();
    let response_headers = upstream.headers().clone();
    let content_type = response_headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let requested_stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let is_sse_response =
        content_type.contains("text/event-stream") || (status.is_success() && requested_stream);

    if is_sse_response {
        let smooth_streaming_text = surface == ApiSurface::OpenAi;
        let profile_id = profile.id.clone();
        let logs = state.logs.clone();
        let cache = state.reasoning_cache.clone();
        let rid = request_id.clone();
        let stream = stream! {
            let mut parser = SseReasoningParser::default();
            let mut smoother = SseTextCoalescer::default();
            let mut last_cached_len = 0usize;
            let mut upstream_stream = upstream.bytes_stream();
            while let Some(chunk) = upstream_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        parser.push(&bytes);
                        if let Some(capture) = parser.capture() {
                            if capture.reasoning.len() > last_cached_len {
                                last_cached_len = capture.reasoning.len();
                                cache_reasoning_capture(&cache, &capture).await;
                                let _ = logs.append(
                                    &profile_id,
                                    "info",
                                    Some(&rid),
                                    format!(
                                        "Cached streamed reasoning for {} tool call(s), {} chars",
                                        capture.tool_ids.len(),
                                        capture.reasoning.len()
                                    ),
                                ).await;
                            }
                        }
                        if smooth_streaming_text {
                            for item in smoother.push(&bytes) {
                                yield Ok::<Bytes, Infallible>(item);
                            }
                        } else {
                            yield Ok::<Bytes, Infallible>(bytes);
                        }
                    }
                    Err(error) => {
                        let _ = logs.append(&profile_id, "error", Some(&rid), format!("SSE upstream error: {error}")).await;
                        break;
                    }
                }
            }
            parser.finish();
            if let Some(capture) = parser.capture() {
                if capture.reasoning.len() > last_cached_len {
                    cache_reasoning_capture(&cache, &capture).await;
                    let _ = logs.append(
                        &profile_id,
                        "info",
                        Some(&rid),
                        format!(
                            "Cached streamed reasoning for {} tool call(s), {} chars",
                            capture.tool_ids.len(),
                            capture.reasoning.len()
                        ),
                    ).await;
                }
            }
            if smooth_streaming_text {
                for item in smoother.finish() {
                    yield Ok::<Bytes, Infallible>(item);
                }
            }
        };
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() =
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        copy_response_headers(response.headers_mut(), &response_headers);
        state
            .logs
            .append(
                &profile.id,
                "info",
                Some(&request_id),
                format!("{method} {} <- upstream {status} stream", uri.path()),
            )
            .await?;
        Ok(response)
    } else {
        let mut response_status = status;
        let mut response_headers = response_headers;
        let mut bytes = upstream.bytes().await?;
        if let Ok(json) = serde_json::from_slice::<Value>(&bytes) {
            cache_reasoning_json(&state.reasoning_cache, &json).await;
            if response_status.is_success()
                && surface == ApiSurface::OpenAi
                && settings.network_request_mcp_enabled()
            {
                if let Some(tool_result) = run_openai_builtin_mcp_tools(
                    &state,
                    &profile,
                    &headers,
                    &request_id,
                    &target,
                    timeout,
                    value.clone(),
                    json,
                )
                .await?
                {
                    response_status = tool_result.status;
                    response_headers = tool_result.headers;
                    bytes = tool_result.body;
                    if let Ok(json) = serde_json::from_slice::<Value>(&bytes) {
                        cache_reasoning_json(&state.reasoning_cache, &json).await;
                    }
                }
            }
        }
        if !response_status.is_success() {
            let body_excerpt = String::from_utf8_lossy(&bytes)
                .chars()
                .take(1200)
                .collect::<String>();
            state
                .logs
                .append(
                    &profile.id,
                    "error",
                    Some(&request_id),
                    format!(
                        "{method} {} upstream error body: {body_excerpt}",
                        uri.path()
                    ),
                )
                .await?;
        }
        let mut response = Response::new(Body::from(bytes));
        *response.status_mut() =
            StatusCode::from_u16(response_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        copy_response_headers(response.headers_mut(), &response_headers);
        state
            .logs
            .append(
                &profile.id,
                "info",
                Some(&request_id),
                format!("{method} {} <- upstream {response_status}", uri.path()),
            )
            .await?;
        Ok(response)
    }
}

#[derive(Clone)]
struct RouteTarget {
    surface: ApiSurface,
    kind: RouteKind,
}

#[derive(Clone)]
enum RouteKind {
    Upstream {
        surface: ApiSurface,
        upstream_path: &'static str,
    },
    LocalModels {
        surface: ApiSurface,
    },
}

fn route_for(path: &str, profile: &GatewayProfile) -> anyhow::Result<RouteTarget> {
    let route = match path {
        "/v1/messages" | "/anthropic/v1/messages" => RouteTarget {
            surface: ApiSurface::Anthropic,
            kind: RouteKind::Upstream {
                surface: ApiSurface::Anthropic,
                upstream_path: "/anthropic/v1/messages",
            },
        },
        "/v1/chat/completions"
        | "/chat/completions"
        | "/anthropic/chat/completions"
        | "/anthropic/v1/chat/completions" => RouteTarget {
            surface: ApiSurface::OpenAi,
            kind: RouteKind::Upstream {
                surface: ApiSurface::OpenAi,
                upstream_path: "/chat/completions",
            },
        },
        "/anthropic/v1/models" => RouteTarget {
            surface: ApiSurface::Anthropic,
            kind: RouteKind::LocalModels {
                surface: ApiSurface::Anthropic,
            },
        },
        "/v1/models" | "/models" => RouteTarget {
            surface: ApiSurface::OpenAi,
            kind: RouteKind::LocalModels {
                surface: ApiSurface::OpenAi,
            },
        },
        _ => return Err(anyhow!("unsupported gateway route: {path}")),
    };
    if !profile.enabled_surfaces.contains(&route.surface) {
        return Err(anyhow!("API surface is disabled for this profile"));
    }
    Ok(route)
}

fn target_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

fn models_response(surface: ApiSurface, profile: &GatewayProfile) -> Value {
    let mut model_ids = vec![
        profile.model_mapping.main.clone(),
        profile.model_mapping.opus.clone(),
        profile.model_mapping.sonnet.clone(),
        profile.model_mapping.haiku.clone(),
        profile.model_mapping.subagent.clone(),
        "deepseek-chat".to_string(),
        "deepseek-reasoner".to_string(),
    ];
    model_ids.sort();
    model_ids.dedup();

    match surface {
        ApiSurface::Anthropic => {
            let data = model_ids
                .into_iter()
                .map(|id| {
                    json!({
                        "id": id,
                        "type": "model",
                        "display_name": id,
                        "created_at": "2026-01-01T00:00:00Z"
                    })
                })
                .collect::<Vec<_>>();
            let first_id = data
                .first()
                .and_then(|item| item.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            let last_id = data
                .last()
                .and_then(|item| item.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            json!({
                "data": data,
                "has_more": false,
                "first_id": first_id,
                "last_id": last_id
            })
        }
        ApiSurface::OpenAi => {
            let data = model_ids
                .into_iter()
                .map(|id| {
                    json!({
                        "id": id,
                        "object": "model",
                        "created": 1767225600,
                        "owned_by": "deepseek"
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "object": "list",
                "data": data
            })
        }
    }
}

async fn normalize_request_body(
    surface: ApiSurface,
    profile: &GatewayProfile,
    value: &mut Value,
    cache: &Arc<Mutex<HashMap<String, String>>>,
) {
    match surface {
        ApiSurface::Anthropic => normalize_anthropic(profile, value, cache).await,
        ApiSurface::OpenAi => normalize_openai(profile, value, cache).await,
    }
}

fn inject_settings_context(surface: ApiSurface, settings: &AppSettings, value: &mut Value) {
    let Some(context) = settings.request_context() else {
        return;
    };
    match surface {
        ApiSurface::Anthropic => inject_anthropic_system_context(value, context),
        ApiSurface::OpenAi => inject_openai_system_context(value, context),
    }
}

fn inject_anthropic_system_context(value: &mut Value, context: String) {
    match value.get_mut("system") {
        Some(Value::String(existing)) => {
            if !existing.contains(&context) {
                existing.push_str("\n\n");
                existing.push_str(&context);
            }
        }
        Some(Value::Array(items)) => {
            items.push(json!({ "type": "text", "text": context }));
        }
        Some(_) => {}
        None => {
            value["system"] = Value::String(context);
        }
    }
}

fn inject_openai_system_context(value: &mut Value, context: String) {
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        value["messages"] = Value::Array(vec![json!({ "role": "system", "content": context })]);
        return;
    };
    if let Some(Value::Object(first)) = messages.first_mut() {
        if first.get("role").and_then(Value::as_str) == Some("system") {
            if let Some(Value::String(content)) = first.get_mut("content") {
                if !content.contains(&context) {
                    content.push_str("\n\n");
                    content.push_str(&context);
                }
                return;
            }
        }
    }
    messages.insert(0, json!({ "role": "system", "content": context }));
}

fn inject_builtin_mcp_tools(surface: ApiSurface, settings: &AppSettings, value: &mut Value) {
    if surface != ApiSurface::OpenAi || !settings.network_request_mcp_enabled() {
        return;
    }
    let tool = network_request_tool_schema();
    match value.get_mut("tools").and_then(Value::as_array_mut) {
        Some(tools) => {
            let exists = tools.iter().any(|item| {
                item.pointer("/function/name").and_then(Value::as_str) == Some("network_request")
            });
            if !exists {
                tools.push(tool);
            }
        }
        None => {
            value["tools"] = Value::Array(vec![tool]);
        }
    }
}

fn network_request_tool_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "network_request",
            "description": "Make an HTTP or HTTPS request and return response status, headers, and body text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Absolute HTTP or HTTPS URL."
                    },
                    "method": {
                        "type": "string",
                        "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"],
                        "description": "HTTP method. Defaults to GET."
                    },
                    "headers": {
                        "type": "object",
                        "additionalProperties": { "type": "string" }
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional request body. JSON should be passed as a string."
                    },
                    "timeoutSeconds": {
                        "type": "number",
                        "minimum": 1,
                        "maximum": 30
                    }
                },
                "required": ["url"],
                "additionalProperties": false
            }
        }
    })
}

async fn normalize_anthropic(
    profile: &GatewayProfile,
    value: &mut Value,
    cache: &Arc<Mutex<HashMap<String, String>>>,
) {
    if profile.features.normalize_adaptive_thinking {
        if value.pointer("/thinking/type").and_then(Value::as_str) == Some("adaptive") {
            value["thinking"]["type"] = Value::String("enabled".into());
        }
    }
    if profile.features.map_effort {
        let effort = value
            .pointer("/output_config/effort")
            .and_then(Value::as_str)
            .map(map_effort)
            .unwrap_or("max");
        value["output_config"]["effort"] = Value::String(effort.into());
    }
    if profile.features.one_m_context_defaults {
        map_anthropic_model(profile, value);
    }
    if profile.features.reasoning_replay {
        replay_anthropic_reasoning(value, cache).await;
    }
}

async fn normalize_openai(
    profile: &GatewayProfile,
    value: &mut Value,
    cache: &Arc<Mutex<HashMap<String, String>>>,
) {
    normalize_openai_roles(value);
    if profile.features.normalize_adaptive_thinking {
        if value.pointer("/thinking/type").and_then(Value::as_str) == Some("adaptive") {
            value["thinking"]["type"] = Value::String("enabled".into());
        }
    }
    if profile.features.map_effort {
        let effort = value
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(map_effort)
            .unwrap_or("high");
        value["reasoning_effort"] = Value::String(effort.into());
    }
    normalize_openai_tool_choice_for_thinking(value);
    if profile.features.one_m_context_defaults {
        map_openai_model(profile, value);
    }
    if profile.features.reasoning_replay {
        replay_openai_reasoning(value, cache).await;
    }
}

fn map_effort(input: &str) -> &'static str {
    match input {
        "max" | "xhigh" | "adaptive" => "max",
        "low" | "medium" | "high" => "high",
        _ => "high",
    }
}

fn normalize_openai_roles(value: &mut Value) {
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        if message.get("role").and_then(Value::as_str) == Some("developer") {
            message["role"] = Value::String("system".into());
        }
    }
}

fn normalize_openai_tool_choice_for_thinking(value: &mut Value) {
    if value.get("tool_choice").is_some_and(Value::is_object) {
        value["tool_choice"] = Value::String("auto".into());
    }
}

fn map_anthropic_model(profile: &GatewayProfile, value: &mut Value) {
    let Some(model) = value.get("model").and_then(Value::as_str) else {
        return;
    };
    let mapped = if model.contains("opus") {
        Some(canonical_model_id(&profile.model_mapping.opus))
    } else if model.contains("sonnet") {
        Some(canonical_model_id(&profile.model_mapping.sonnet))
    } else if model.contains("haiku") {
        Some(canonical_model_id(&profile.model_mapping.haiku))
    } else if model == "deepseek-chat"
        || model == "deepseek-reasoner"
        || model.starts_with("deepseek-v4-")
    {
        Some(canonical_model_id(model))
    } else {
        None
    };
    if let Some(mapped) = mapped {
        value["model"] = Value::String(mapped);
    }
}

fn map_openai_model(profile: &GatewayProfile, value: &mut Value) {
    let Some(model) = value.get("model").and_then(Value::as_str) else {
        return;
    };
    let mapped = if model == "deepseek-chat" || model == "deepseek-reasoner" {
        canonical_model_id(&profile.model_mapping.main)
    } else {
        canonical_model_id(model)
    };
    value["model"] = Value::String(mapped);
}

struct BuiltinMcpToolResult {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Debug)]
struct OpenAiToolCall {
    id: String,
    arguments: Value,
}

async fn run_openai_builtin_mcp_tools(
    state: &GatewayState,
    profile: &GatewayProfile,
    original_headers: &HeaderMap,
    request_id: &str,
    target: &str,
    timeout: Duration,
    mut request_body: Value,
    mut current_json: Value,
) -> anyhow::Result<Option<BuiltinMcpToolResult>> {
    let mut executed_any = false;
    for _ in 0..3 {
        let tool_calls = openai_network_request_tool_calls(&current_json);
        if tool_calls.is_empty() {
            if executed_any {
                return Ok(Some(BuiltinMcpToolResult {
                    status: StatusCode::OK,
                    headers: json_content_headers(),
                    body: Bytes::from(current_json.to_string()),
                }));
            }
            return Ok(None);
        }
        executed_any = true;

        let Some(messages) = request_body
            .get_mut("messages")
            .and_then(Value::as_array_mut)
        else {
            return Err(anyhow!("OpenAI MCP tool loop requires a messages array"));
        };
        if let Some(message) = current_json.pointer("/choices/0/message").cloned() {
            messages.push(message);
        }
        for call in tool_calls {
            let result = execute_network_request_tool(&state.client, &call.arguments).await;
            let content = serde_json::to_string(&result)?;
            state
                .logs
                .append(
                    &profile.id,
                    "info",
                    Some(request_id),
                    format!("Executed builtin MCP tool network_request for {}", call.id),
                )
                .await?;
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": content
            }));
        }

        let request = state
            .client
            .post(target)
            .timeout(timeout)
            .json(&request_body);
        let (request, _) = apply_forwarded_headers(request, original_headers, profile);
        let upstream = request.send().await?;
        let status = upstream.status();
        let headers = upstream.headers().clone();
        let body = upstream.bytes().await?;
        if !status.is_success() {
            return Ok(Some(BuiltinMcpToolResult {
                status,
                headers,
                body,
            }));
        }
        current_json = serde_json::from_slice::<Value>(&body)
            .context("MCP tool follow-up response must be JSON")?;
    }

    Ok(Some(BuiltinMcpToolResult {
        status: StatusCode::BAD_GATEWAY,
        headers: json_content_headers(),
        body: Bytes::from(
            json!({
                "error": {
                    "message": "MCP tool loop limit reached",
                    "type": "deepseek_gateway_error"
                }
            })
            .to_string(),
        ),
    }))
}

fn openai_network_request_tool_calls(value: &Value) -> Vec<OpenAiToolCall> {
    value
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|call| {
            call.pointer("/function/name").and_then(Value::as_str) == Some("network_request")
        })
        .filter_map(|call| {
            let id = call.get("id").and_then(Value::as_str)?.to_string();
            let raw_arguments = call.pointer("/function/arguments")?;
            let arguments = match raw_arguments {
                Value::String(text) => serde_json::from_str::<Value>(text).unwrap_or_else(|_| {
                    json!({
                        "url": text
                    })
                }),
                other => other.clone(),
            };
            Some(OpenAiToolCall { id, arguments })
        })
        .collect()
}

async fn execute_network_request_tool(client: &Client, arguments: &Value) -> Value {
    match execute_network_request_tool_inner(client, arguments).await {
        Ok(value) => value,
        Err(error) => json!({
            "ok": false,
            "error": error.to_string()
        }),
    }
}

async fn execute_network_request_tool_inner(
    client: &Client,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let url = arguments
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("network_request.url is required"))?;
    let parsed = reqwest::Url::parse(url).context("network_request.url must be absolute")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(anyhow!("network_request only supports http and https URLs"));
    }

    let method = arguments
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_ascii_uppercase();
    let method = reqwest::Method::from_bytes(method.as_bytes())
        .context("network_request.method is invalid")?;
    if !matches!(
        method,
        reqwest::Method::GET
            | reqwest::Method::POST
            | reqwest::Method::PUT
            | reqwest::Method::PATCH
            | reqwest::Method::DELETE
            | reqwest::Method::HEAD
    ) {
        return Err(anyhow!("network_request.method is not allowed"));
    }

    let timeout = arguments
        .get("timeoutSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(15)
        .clamp(1, 30);
    let mut request = client
        .request(method, parsed)
        .timeout(Duration::from_secs(timeout));
    if let Some(headers) = arguments.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
                    continue;
                };
                if should_forward_header(&name) {
                    request = request.header(name, value);
                }
            }
        }
    }
    if let Some(body) = arguments.get("body") {
        if let Some(text) = body.as_str() {
            request = request.body(text.to_string());
        } else if !body.is_null() {
            request = request.json(body);
        }
    }

    let response = request.send().await?;
    let status = response.status();
    let final_url = response.url().to_string();
    let headers = response
        .headers()
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "set-cookie" | "authorization" | "x-api-key"))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), Value::String(value.to_string())))
        })
        .collect::<serde_json::Map<String, Value>>();
    let bytes = response.bytes().await?;
    let body = String::from_utf8_lossy(&bytes);
    let max_chars = 65_536usize;
    let body_excerpt = body.chars().take(max_chars).collect::<String>();
    let truncated = body.chars().count() > max_chars;

    Ok(json!({
        "ok": status.is_success(),
        "status": status.as_u16(),
        "url": final_url,
        "headers": headers,
        "body": body_excerpt,
        "truncated": truncated
    }))
}

async fn replay_anthropic_reasoning(
    value: &mut Value,
    cache: &Arc<Mutex<HashMap<String, String>>>,
) {
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let cache = cache.lock().await;
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let has_thinking = content
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some("thinking"));
        if has_thinking {
            continue;
        }
        let tool_ids = content
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(|item| item.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if !tool_ids.is_empty() {
            if let Some(reasoning) = tool_ids
                .iter()
                .find_map(|id| cache.get(*id).cloned())
                .or_else(|| cache.get(LATEST_TOOL_REASONING_KEY).cloned())
            {
                content.insert(0, json!({ "type": "thinking", "thinking": reasoning }));
            }
        }
    }
}

async fn replay_openai_reasoning(value: &mut Value, cache: &Arc<Mutex<HashMap<String, String>>>) {
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let cache = cache.lock().await;
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let has_reasoning = message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_some_and(|reasoning| !reasoning.is_empty());
        if has_reasoning {
            continue;
        }
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
            if calls.is_empty() {
                continue;
            }
            if let Some(reasoning) = calls
                .iter()
                .filter_map(|call| call.get("id").and_then(Value::as_str))
                .find_map(|id| cache.get(id).cloned())
                .or_else(|| cache.get(LATEST_TOOL_REASONING_KEY).cloned())
            {
                message["reasoning_content"] = Value::String(reasoning);
            }
        }
    }
}

fn should_forward_header(name: &HeaderName) -> bool {
    !matches!(
        name.as_str(),
        "host" | "content-length" | "connection" | "accept-encoding"
    )
}

fn apply_forwarded_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HeaderMap,
    profile: &GatewayProfile,
) -> (reqwest::RequestBuilder, bool) {
    let has_request_api_key = headers_have_api_key(headers);
    for (name, value) in headers.iter() {
        if should_forward_header(name) && !is_empty_api_key_header(name, value) {
            request = request.header(name, value);
        }
    }
    if !has_request_api_key {
        if let Some(api_key) = profile.fallback_api_key() {
            request = request.header(header::AUTHORIZATION, format!("Bearer {api_key}"));
            return (request, true);
        }
    }
    (request, false)
}

fn headers_have_api_key(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::AUTHORIZATION)
        .iter()
        .any(header_value_has_api_key)
        || headers
            .get_all("x-api-key")
            .iter()
            .any(header_value_has_api_key)
}

fn is_empty_api_key_header(name: &HeaderName, value: &HeaderValue) -> bool {
    matches!(name.as_str(), "authorization" | "x-api-key") && !header_value_has_api_key(value)
}

fn header_value_has_api_key(value: &HeaderValue) -> bool {
    let Ok(raw) = value.to_str() else {
        return false;
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return false;
    }
    if raw.eq_ignore_ascii_case("bearer") {
        return false;
    }
    if raw.to_ascii_lowercase().starts_with("bearer ") {
        let token = raw[7..].trim();
        return token.len() >= 3 && !looks_like_placeholder_api_key(token);
    }
    raw.len() >= 3 && !looks_like_placeholder_api_key(raw)
}

fn looks_like_placeholder_api_key(value: &str) -> bool {
    let normalized = value
        .trim()
        .trim_matches(|ch| ch == '<' || ch == '>' || ch == '"' || ch == '\'')
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "dummy"
            | "placeholder"
            | "changeme"
            | "change-me"
            | "your-api-key"
            | "your_api_key"
            | "api-key"
            | "api_key"
            | "none"
            | "null"
            | "undefined"
    ) || (normalized.contains("your") && normalized.contains("key"))
}

fn json_content_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers
}

fn copy_response_headers(target: &mut HeaderMap, source: &HeaderMap) {
    for (name, value) in source {
        if matches!(
            name.as_str(),
            "content-length" | "connection" | "transfer-encoding"
        ) {
            continue;
        }
        target.insert(name, value.clone());
    }
}

fn json_response(status: StatusCode, value: Value) -> Response<Body> {
    let mut response = Response::new(Body::from(value.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

#[derive(Debug, Default)]
struct SseTextCoalescer {
    pending: Vec<u8>,
    buffered: Option<BufferedTextEvent>,
    reasoning_buffered: Option<BufferedTextEvent>,
}

#[derive(Debug)]
struct BufferedTextEvent {
    value: Value,
    content: String,
}

impl SseTextCoalescer {
    fn push(&mut self, bytes: &[u8]) -> Vec<Bytes> {
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        while let Some(idx) = find_sse_delimiter(&self.pending) {
            let frame = self.pending[..idx].to_vec();
            self.pending.drain(..idx + 2);
            output.extend(self.process_frame(frame));
        }
        output
    }

    fn finish(&mut self) -> Vec<Bytes> {
        let mut output = Vec::new();
        output.extend(self.flush_all_buffered());
        if !self.pending.is_empty() {
            output.push(Bytes::from(std::mem::take(&mut self.pending)));
        }
        output
    }

    fn process_frame(&mut self, frame: Vec<u8>) -> Vec<Bytes> {
        let Ok(text) = String::from_utf8(frame.clone()) else {
            let mut output = self.flush_all_buffered();
            output.push(frame_with_delimiter(frame));
            return output;
        };
        let Some(data) = sse_data_payload(&text) else {
            let mut output = self.flush_all_buffered();
            output.push(frame_with_delimiter(frame));
            return output;
        };
        if data == "[DONE]" {
            let mut output = self.flush_all_buffered();
            output.push(frame_with_delimiter(frame));
            return output;
        }
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            let mut output = self.flush_all_buffered();
            output.push(frame_with_delimiter(frame));
            return output;
        };
        if let Some(content) = openai_content_delta(&value) {
            if content.is_empty() {
                return Vec::new();
            }
            let content = content.to_string();
            match &mut self.buffered {
                Some(buffered) => buffered.content.push_str(&content),
                None => {
                    self.buffered = Some(BufferedTextEvent { value, content });
                }
            }
            if self.should_flush_buffered() {
                return self.flush_buffered();
            }
            return Vec::new();
        }
        if let Some(reasoning) = openai_reasoning_delta(&value) {
            if reasoning.is_empty() {
                return Vec::new();
            }
            if self.buffered.is_some() {
                return vec![frame_with_delimiter(frame)];
            }
            let reasoning = reasoning.to_string();
            match &mut self.reasoning_buffered {
                Some(buffered) => buffered.content.push_str(&reasoning),
                None => {
                    self.reasoning_buffered = Some(BufferedTextEvent {
                        value,
                        content: reasoning,
                    });
                }
            }
            if self.should_flush_reasoning_buffered() {
                return self.flush_reasoning_buffered();
            }
            return Vec::new();
        }

        let mut output = self.flush_all_buffered();
        output.push(frame_with_delimiter(frame));
        output
    }

    fn should_flush_buffered(&self) -> bool {
        let Some(buffered) = &self.buffered else {
            return false;
        };
        let chars = buffered.content.chars().count();
        chars >= 80
            || buffered.content.chars().last().is_some_and(|ch| {
                matches!(
                    ch,
                    '\n' | '。'
                        | '！'
                        | '？'
                        | '，'
                        | '；'
                        | '：'
                        | '.'
                        | '!'
                        | '?'
                        | ','
                        | ';'
                        | ':'
                )
            })
    }

    fn flush_buffered(&mut self) -> Vec<Bytes> {
        let Some(mut buffered) = self.buffered.take() else {
            return Vec::new();
        };
        if let Some(content) = buffered.value.pointer_mut("/choices/0/delta/content") {
            *content = Value::String(buffered.content);
        }
        match serde_json::to_string(&buffered.value) {
            Ok(data) => vec![Bytes::from(format!("data: {data}\n\n"))],
            Err(_) => Vec::new(),
        }
    }

    fn should_flush_reasoning_buffered(&self) -> bool {
        let Some(buffered) = &self.reasoning_buffered else {
            return false;
        };
        buffered.content.chars().count() >= 120
            || buffered
                .content
                .chars()
                .last()
                .is_some_and(|ch| matches!(ch, '\n' | '。' | '！' | '？' | '.' | '!' | '?'))
    }

    fn flush_reasoning_buffered(&mut self) -> Vec<Bytes> {
        let Some(mut buffered) = self.reasoning_buffered.take() else {
            return Vec::new();
        };
        if let Some(reasoning) = buffered
            .value
            .pointer_mut("/choices/0/delta/reasoning_content")
        {
            *reasoning = Value::String(buffered.content);
        }
        match serde_json::to_string(&buffered.value) {
            Ok(data) => vec![Bytes::from(format!("data: {data}\n\n"))],
            Err(_) => Vec::new(),
        }
    }

    fn flush_all_buffered(&mut self) -> Vec<Bytes> {
        let mut output = self.flush_reasoning_buffered();
        output.extend(self.flush_buffered());
        output
    }
}

fn find_sse_delimiter(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|window| window == b"\n\n")
}

fn frame_with_delimiter(mut frame: Vec<u8>) -> Bytes {
    frame.extend_from_slice(b"\n\n");
    Bytes::from(frame)
}

fn sse_data_payload(frame: &str) -> Option<String> {
    let data = frame
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("data:"))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

fn openai_content_delta(value: &Value) -> Option<&str> {
    let choices = value.get("choices")?.as_array()?;
    if choices.len() != 1 {
        return None;
    }
    let choice = &choices[0];
    if choice
        .get("finish_reason")
        .is_some_and(|finish| !finish.is_null())
    {
        return None;
    }
    let delta = choice.get("delta")?.as_object()?;
    if delta.contains_key("tool_calls")
        || delta
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_some_and(|reasoning| !reasoning.is_empty())
    {
        return None;
    }
    delta.get("content").and_then(Value::as_str)
}

fn openai_reasoning_delta(value: &Value) -> Option<&str> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| {
            if choices.len() != 1 {
                return None;
            }
            choices[0]
                .pointer("/delta/reasoning_content")
                .and_then(Value::as_str)
        })
}

#[derive(Debug, Default)]
struct SseReasoningParser {
    pending: String,
    reasoning: String,
    tool_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamReasoningCapture {
    tool_ids: Vec<String>,
    reasoning: String,
}

impl SseReasoningParser {
    fn push(&mut self, bytes: &[u8]) {
        self.pending.push_str(&String::from_utf8_lossy(bytes));
        while let Some(idx) = self.pending.find("\n\n") {
            let frame = self.pending[..idx].to_string();
            self.pending = self.pending[idx + 2..].to_string();
            if let Some(value) = parse_sse_frame(&frame) {
                self.ingest_value(&value);
            }
        }
    }

    fn finish(&mut self) {
        if self.pending.trim().is_empty() {
            return;
        }
        let frame = std::mem::take(&mut self.pending);
        if let Some(value) = parse_sse_frame(&frame) {
            self.ingest_value(&value);
        }
    }

    fn capture(&self) -> Option<StreamReasoningCapture> {
        if self.reasoning.is_empty() || self.tool_ids.is_empty() {
            return None;
        }
        Some(StreamReasoningCapture {
            tool_ids: self.tool_ids.clone(),
            reasoning: self.reasoning.clone(),
        })
    }

    fn ingest_value(&mut self, value: &Value) {
        append_stream_reasoning(value, &mut self.reasoning);
        collect_stream_tool_ids(value, &mut self.tool_ids);
    }
}

fn parse_sse_frame(frame: &str) -> Option<Value> {
    let data = frame
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("data:"))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    serde_json::from_str(&data).ok()
}

fn append_stream_reasoning(value: &Value, reasoning: &mut String) {
    for pointer in [
        "/delta/thinking",
        "/delta/reasoning_content",
        "/content/0/thinking",
        "/content_block/thinking",
    ] {
        if let Some(chunk) = value.pointer(pointer).and_then(Value::as_str) {
            reasoning.push_str(chunk);
        }
    }
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(chunk) = choice
                .pointer("/delta/reasoning_content")
                .and_then(Value::as_str)
            {
                reasoning.push_str(chunk);
            }
        }
    }
}

fn collect_stream_tool_ids(value: &Value, tool_ids: &mut Vec<String>) {
    for pointer in ["/delta/id", "/content/0/id", "/content_block/id"] {
        if let Some(id) = value.pointer(pointer).and_then(Value::as_str) {
            push_unique_tool_id(tool_ids, id);
        }
    }
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let Some(calls) = choice
                .pointer("/delta/tool_calls")
                .and_then(Value::as_array)
            else {
                continue;
            };
            for call in calls {
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    push_unique_tool_id(tool_ids, id);
                }
            }
        }
    }
}

fn push_unique_tool_id(tool_ids: &mut Vec<String>, id: &str) {
    if !id.is_empty() && !tool_ids.iter().any(|existing| existing == id) {
        tool_ids.push(id.to_string());
    }
}

async fn cache_reasoning_capture(
    cache: &Arc<Mutex<HashMap<String, String>>>,
    capture: &StreamReasoningCapture,
) {
    let mut cache = cache.lock().await;
    for tool_id in &capture.tool_ids {
        cache.insert(tool_id.clone(), capture.reasoning.clone());
    }
    cache.insert(
        LATEST_TOOL_REASONING_KEY.to_string(),
        capture.reasoning.clone(),
    );
}

async fn cache_reasoning_json(cache: &Arc<Mutex<HashMap<String, String>>>, value: &Value) {
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        let mut cache = cache.lock().await;
        for choice in choices {
            if let Some(message) = choice.get("message") {
                let reasoning = message.get("reasoning_content").and_then(Value::as_str);
                let calls = message.get("tool_calls").and_then(Value::as_array);
                if let (Some(reasoning), Some(calls)) = (reasoning, calls) {
                    cache.insert(LATEST_TOOL_REASONING_KEY.to_string(), reasoning.to_string());
                    for id in calls
                        .iter()
                        .filter_map(|call| call.get("id").and_then(Value::as_str))
                    {
                        cache.insert(id.to_string(), reasoning.to_string());
                    }
                }
            }
        }
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let reasoning = content
            .iter()
            .find(|item| item.get("type").and_then(Value::as_str) == Some("thinking"))
            .and_then(|item| item.get("thinking").and_then(Value::as_str));
        if let Some(reasoning) = reasoning {
            let mut cache = cache.lock().await;
            cache.insert(LATEST_TOOL_REASONING_KEY.to_string(), reasoning.to_string());
            for id in content
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
                .filter_map(|item| item.get("id").and_then(Value::as_str))
            {
                cache.insert(id.to_string(), reasoning.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppSettings, GatewayProfile, SkillConfig};
    use crate::storage::SettingsStore;
    use wiremock::{
        matchers::{body_json, body_string_contains, header as header_match, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn settings_store() -> SettingsStore {
        let dir = std::env::temp_dir().join(format!("dsp-settings-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"mcpConfig":{"mcpServers":{}},"skills":[]}"#,
        )
        .unwrap();
        SettingsStore::new(dir)
    }

    fn network_settings_store() -> SettingsStore {
        SettingsStore::new(tempfile::tempdir().unwrap().keep())
    }

    #[tokio::test]
    async fn normalizes_adaptive_thinking_for_anthropic() {
        let profile = GatewayProfile::new_default("p".into(), "P".into(), 17777);
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let mut value = json!({
            "model": "claude-opus-4-6",
            "thinking": { "type": "adaptive" },
            "messages": []
        });
        normalize_request_body(ApiSurface::Anthropic, &profile, &mut value, &cache).await;
        assert_eq!(
            value.pointer("/thinking/type").and_then(Value::as_str),
            Some("enabled")
        );
        assert_eq!(
            value
                .pointer("/output_config/effort")
                .and_then(Value::as_str),
            Some("max")
        );
        assert_eq!(
            value.get("model").and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
    }

    #[tokio::test]
    async fn maps_openai_effort_and_legacy_model() {
        let profile = GatewayProfile::new_default("p".into(), "P".into(), 17777);
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let mut value = json!({
            "model": "deepseek-reasoner",
            "reasoning_effort": "medium",
            "messages": [{ "role": "developer", "content": "style guide" }],
            "tool_choice": {
                "type": "function",
                "function": { "name": "lookup" }
            }
        });
        normalize_request_body(ApiSurface::OpenAi, &profile, &mut value, &cache).await;
        assert_eq!(
            value.get("reasoning_effort").and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            value.get("model").and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            value.pointer("/messages/0/role").and_then(Value::as_str),
            Some("system")
        );
        assert_eq!(
            value.get("tool_choice").and_then(Value::as_str),
            Some("auto")
        );
    }

    #[tokio::test]
    async fn replays_reasoning_only_for_tool_turns() {
        let profile = GatewayProfile::new_default("p".into(), "P".into(), 17777);
        let cache = Arc::new(Mutex::new(HashMap::from([(
            "toolu_1".into(),
            "saved thinking".into(),
        )])));
        let mut value = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {
                    "role": "assistant",
                    "content": [{ "type": "tool_use", "id": "toolu_1", "name": "read", "input": {} }]
                },
                {
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "No tools here" }]
                }
            ]
        });
        normalize_request_body(ApiSurface::Anthropic, &profile, &mut value, &cache).await;
        let first = value
            .pointer("/messages/0/content/0/type")
            .and_then(Value::as_str);
        let second = value
            .pointer("/messages/1/content/0/type")
            .and_then(Value::as_str);
        assert_eq!(first, Some("thinking"));
        assert_eq!(second, Some("text"));
    }

    #[tokio::test]
    async fn replays_latest_reasoning_when_client_rewrites_tool_call_id() {
        let profile = GatewayProfile::new_default("p".into(), "P".into(), 17777);
        let cache = Arc::new(Mutex::new(HashMap::from([(
            LATEST_TOOL_REASONING_KEY.into(),
            "latest thinking".into(),
        )])));
        let mut value = json!({
            "model": "deepseek-reasoner",
            "messages": [
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "client_rewritten_id",
                        "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }]
                },
                {
                    "role": "assistant",
                    "content": "No tools here"
                }
            ]
        });

        normalize_request_body(ApiSurface::OpenAi, &profile, &mut value, &cache).await;

        assert_eq!(
            value
                .pointer("/messages/0/reasoning_content")
                .and_then(Value::as_str),
            Some("latest thinking")
        );
        assert!(value.pointer("/messages/1/reasoning_content").is_none());
    }

    #[test]
    fn maps_routes() {
        let profile = GatewayProfile::new_default("p".into(), "P".into(), 17777);
        let messages = route_for("/v1/messages", &profile).unwrap();
        let chat = route_for("/chat/completions", &profile).unwrap();
        let android_studio_chat = route_for("/anthropic/chat/completions", &profile).unwrap();
        let models = route_for("/anthropic/v1/models", &profile).unwrap();
        assert!(matches!(
            messages.kind,
            RouteKind::Upstream {
                upstream_path: "/anthropic/v1/messages",
                ..
            }
        ));
        assert!(matches!(
            chat.kind,
            RouteKind::Upstream {
                upstream_path: "/chat/completions",
                ..
            }
        ));
        assert!(matches!(
            android_studio_chat.kind,
            RouteKind::Upstream {
                upstream_path: "/chat/completions",
                ..
            }
        ));
        assert!(matches!(
            models.kind,
            RouteKind::LocalModels {
                surface: ApiSurface::Anthropic
            }
        ));
        assert!(route_for("/unknown", &profile).is_err());
    }

    #[test]
    fn detects_empty_api_key_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer"));
        assert!(!headers_have_api_key(&headers));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-client"),
        );
        assert!(headers_have_api_key(&headers));

        let empty = HeaderValue::from_static("   ");
        assert!(is_empty_api_key_header(&header::AUTHORIZATION, &empty));
        let placeholder = HeaderValue::from_static("dummy");
        assert!(is_empty_api_key_header(
            &HeaderName::from_static("x-api-key"),
            &placeholder
        ));
    }

    #[test]
    fn injects_settings_context_for_anthropic_and_openai() {
        let settings = AppSettings {
            mcp_config: json!({
                "mcpServers": {
                    "Filesystem": {
                        "command": "npx",
                        "args": ["@modelcontextprotocol/server-filesystem"],
                        "description": "local files",
                        "enabled": true
                    }
                }
            }),
            mcp_services: Vec::new(),
            skills: vec![SkillConfig {
                id: "skill-1".into(),
                name: "Android Studio".into(),
                description: "IDE support".into(),
                instructions: "Prefer Android Studio AI compatible answers.".into(),
                enabled: true,
            }],
        };
        let mut anthropic = json!({ "model": "deepseek-chat", "messages": [] });
        inject_settings_context(ApiSurface::Anthropic, &settings, &mut anthropic);
        assert!(anthropic
            .get("system")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("Android Studio") && text.contains("Filesystem")));

        let mut openai =
            json!({ "model": "deepseek-chat", "messages": [{ "role": "user", "content": "hi" }] });
        inject_settings_context(ApiSurface::OpenAi, &settings, &mut openai);
        assert_eq!(
            openai.pointer("/messages/0/role").and_then(Value::as_str),
            Some("system")
        );
        assert!(openai
            .pointer("/messages/0/content")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("Android Studio") && text.contains("MCP")));
    }

    #[test]
    fn injects_network_request_tool_schema_for_openai() {
        let settings = AppSettings::default();
        let mut openai = json!({
            "model": "deepseek-chat",
            "messages": [{ "role": "user", "content": "fetch a page" }]
        });
        inject_builtin_mcp_tools(ApiSurface::OpenAi, &settings, &mut openai);

        assert_eq!(
            openai
                .pointer("/tools/0/function/name")
                .and_then(Value::as_str),
            Some("network_request")
        );
    }

    #[test]
    fn parses_sse_reasoning_with_partial_chunks_and_done() {
        let mut parser = SseReasoningParser::default();
        parser.push(b"data: {\"delta\":{\"id\":\"toolu_1\",");
        parser.push(b"\"thinking\":\"hello\"}}\n\ndata: [DONE]\n\n");
        assert_eq!(
            parser.capture(),
            Some(StreamReasoningCapture {
                tool_ids: vec!["toolu_1".into()],
                reasoning: "hello".into()
            })
        );
        parser.finish();
    }

    #[test]
    fn ignores_malformed_sse_frames() {
        let mut parser = SseReasoningParser::default();
        parser.push(b": keepalive\n\ndata: nope\n\n");
        assert_eq!(parser.capture(), None);
    }

    #[test]
    fn caches_stream_reasoning_when_tool_id_arrives_after_reasoning() {
        let mut parser = SseReasoningParser::default();
        parser.push(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"streamed \"}}]}\n\n");
        parser.push(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}]}\n\n");
        parser.push(
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\"}]}}]}\n\n",
        );
        parser.push(b"data: [DONE]\n\n");

        assert_eq!(
            parser.capture(),
            Some(StreamReasoningCapture {
                tool_ids: vec!["call_1".into()],
                reasoning: "streamed thinking".into()
            })
        );
    }

    #[test]
    fn coalesces_openai_content_delta_chunks_without_changing_text() {
        let mut coalescer = SseTextCoalescer::default();
        let mut output = Vec::new();
        output.extend(coalescer.push(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"Cl\",\"reasoning_content\":null}}]}\n\n",
        ));
        output.extend(coalescer.push(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"aude\",\"reasoning_content\":null}}]}\n\n",
        ));
        assert!(output.is_empty());
        output.extend(coalescer.push(b"data: [DONE]\n\n"));

        let raw = String::from_utf8(
            output
                .into_iter()
                .flat_map(|bytes| bytes.to_vec())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(raw.contains("\"content\":\"Claude\""));
        assert!(raw.contains("data: [DONE]"));
    }

    #[test]
    fn flushes_coalesced_text_before_tool_call_sse_frame() {
        let mut coalescer = SseTextCoalescer::default();
        let mut output = Vec::new();
        output.extend(
            coalescer.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"Deep\"}}]}\n\n"),
        );
        output.extend(
            coalescer.push(
                b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_1\"}]}}]}\n\n",
            ),
        );
        let raw = String::from_utf8(
            output
                .into_iter()
                .flat_map(|bytes| bytes.to_vec())
                .collect::<Vec<_>>(),
        )
        .unwrap();

        assert!(raw.contains("\"content\":\"Deep\""));
        assert!(raw.contains("\"tool_calls\":[{\"id\":\"call_1\"}]"));
    }

    #[test]
    fn reasoning_delta_does_not_split_coalesced_visible_text() {
        let mut coalescer = SseTextCoalescer::default();
        let mut output = Vec::new();
        output
            .extend(coalescer.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"Cl\"}}]}\n\n"));
        output.extend(
            coalescer
                .push(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hidden\"}}]}\n\n"),
        );
        output.extend(
            coalescer.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"aude\"}}]}\n\n"),
        );
        output.extend(coalescer.push(b"data: [DONE]\n\n"));
        let raw = String::from_utf8(
            output
                .into_iter()
                .flat_map(|bytes| bytes.to_vec())
                .collect::<Vec<_>>(),
        )
        .unwrap();

        assert!(raw.contains("\"reasoning_content\":\"hidden\""));
        assert!(raw.contains("\"content\":\"Claude\""));
        assert_eq!(raw.matches("\"content\":\"").count(), 1);
    }

    #[test]
    fn coalesces_reasoning_delta_chunks_for_clients_that_render_thinking() {
        let mut coalescer = SseTextCoalescer::default();
        let mut output = Vec::new();
        output.extend(coalescer.push(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"之前\"}}]}\n\n".as_bytes(),
        ));
        output.extend(coalescer.push(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"已经\"}}]}\n\n".as_bytes(),
        ));
        output.extend(coalescer.push(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"全面\"}}]}\n\n".as_bytes(),
        ));
        output.extend(coalescer.push(b"data: [DONE]\n\n"));
        let raw = String::from_utf8(
            output
                .into_iter()
                .flat_map(|bytes| bytes.to_vec())
                .collect::<Vec<_>>(),
        )
        .unwrap();

        assert!(raw.contains("\"reasoning_content\":\"之前已经全面\""));
        assert_eq!(raw.matches("\"reasoning_content\":\"").count(), 1);
    }

    #[tokio::test]
    async fn proxies_anthropic_sse_and_normalizes_claude_code_body() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "thinking": { "type": "enabled" },
                "output_config": { "effort": "max" },
                "messages": []
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("data: {\"delta\":{\"id\":\"toolu_1\",\"thinking\":\"hello\"}}\n\ndata: [DONE]\n\n"),
            )
            .mount(&upstream)
            .await;

        let port = portpicker::pick_unused_port().unwrap();
        let mut profile = GatewayProfile::new_default("sse".into(), "SSE".into(), port);
        profile.upstream_base_url = upstream.uri();
        let logs = LogStore::new(tempfile::tempdir().unwrap().path().into());
        let registry = GatewayRegistry::start(profile, logs, settings_store())
            .await
            .unwrap();

        let response = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/v1/messages"))
            .json(&json!({
                "model": "claude-opus-4-6",
                "thinking": { "type": "adaptive" },
                "messages": []
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(response
            .text()
            .await
            .unwrap()
            .contains("\"thinking\":\"hello\""));
        registry.stop().await;
    }

    #[tokio::test]
    async fn serves_anthropic_models_locally_for_client_refresh() {
        let port = portpicker::pick_unused_port().unwrap();
        let profile = GatewayProfile::new_default("models".into(), "Models".into(), port);
        let tempdir = tempfile::tempdir().unwrap();
        let logs = LogStore::new(tempdir.path().into());
        let registry = GatewayRegistry::start(profile, logs.clone(), settings_store())
            .await
            .unwrap();

        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/anthropic/v1/models"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = response.json::<Value>().await.unwrap();
        assert_eq!(body.get("has_more").and_then(Value::as_bool), Some(false));
        let ids = body
            .get("data")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|item| item.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(ids.contains(&"deepseek-v4-pro[1m]"));
        assert!(ids.contains(&"deepseek-v4-flash"));

        let entries = logs.read("models", 20).await.unwrap();
        assert!(entries.iter().any(|entry| entry
            .message
            .contains("GET /anthropic/v1/models -> local models 200")));
        registry.stop().await;
    }

    #[tokio::test]
    async fn logs_unsupported_routes() {
        let port = portpicker::pick_unused_port().unwrap();
        let profile = GatewayProfile::new_default("bad-route".into(), "Bad Route".into(), port);
        let tempdir = tempfile::tempdir().unwrap();
        let logs = LogStore::new(tempdir.path().into());
        let registry = GatewayRegistry::start(profile, logs.clone(), settings_store())
            .await
            .unwrap();

        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/anthropic/v1/unknown"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
        let entries = logs.read("bad-route", 20).await.unwrap();
        assert!(entries
            .iter()
            .any(|entry| entry.message.contains("Received GET /anthropic/v1/unknown")));
        assert!(entries
            .iter()
            .any(|entry| entry.message.contains("unsupported gateway route")));
        registry.stop().await;
    }

    #[tokio::test]
    async fn injects_profile_api_key_only_when_request_has_none() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header_match("authorization", "Bearer sk-profile"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "fallback key" }],
                "reasoning_effort": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
            })))
            .expect(1)
            .mount(&upstream)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header_match("authorization", "Bearer sk-client"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "client key" }],
                "reasoning_effort": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
            })))
            .expect(1)
            .mount(&upstream)
            .await;

        let port = portpicker::pick_unused_port().unwrap();
        let mut profile = GatewayProfile::new_default("keys".into(), "Keys".into(), port);
        profile.upstream_base_url = upstream.uri();
        profile.api_key = Some("sk-profile".into());
        let logs = LogStore::new(tempfile::tempdir().unwrap().path().into());
        let registry = GatewayRegistry::start(profile, logs, settings_store())
            .await
            .unwrap();
        let client = reqwest::Client::new();

        let fallback = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "fallback key" }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(fallback.status(), reqwest::StatusCode::OK);

        let preserved = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .header("authorization", "Bearer sk-client")
            .json(&json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "client key" }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(preserved.status(), reqwest::StatusCode::OK);
        registry.stop().await;
    }

    #[tokio::test]
    async fn executes_builtin_network_request_tool_for_openai_non_streaming() {
        let upstream = MockServer::start().await;
        let network_url = format!("{}/resource", upstream.uri());
        Mock::given(method("GET"))
            .and(path("/resource"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/plain")
                    .set_body_string("network ok"),
            )
            .mount(&upstream)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("fetch network"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "reasoning_content": "need network",
                        "tool_calls": [{
                            "id": "call_network",
                            "type": "function",
                            "function": {
                                "name": "network_request",
                                "arguments": serde_json::to_string(&json!({
                                    "url": network_url,
                                    "method": "GET"
                                })).unwrap()
                            }
                        }]
                    }
                }]
            })))
            .expect(1)
            .with_priority(5)
            .mount(&upstream)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("network ok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "final after network"
                    }
                }]
            })))
            .expect(1)
            .with_priority(1)
            .mount(&upstream)
            .await;

        let port = portpicker::pick_unused_port().unwrap();
        let mut profile = GatewayProfile::new_default("network".into(), "Network".into(), port);
        profile.upstream_base_url = upstream.uri();
        let logs = LogStore::new(tempfile::tempdir().unwrap().path().into());
        let registry = GatewayRegistry::start(profile, logs, network_settings_store())
            .await
            .unwrap();

        let response = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-chat",
                "messages": [{ "role": "user", "content": "fetch network" }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(response
            .text()
            .await
            .unwrap()
            .contains("final after network"));
        registry.stop().await;
    }

    #[tokio::test]
    async fn replays_openai_reasoning_after_tool_call_response() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "call a tool" }],
                "reasoning_effort": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "reasoning_content": "saved reasoning",
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": { "name": "lookup", "arguments": "{}" }
                        }]
                    }
                }]
            })))
            .mount(&upstream)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "saved reasoning",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }]
                }],
                "reasoning_effort": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
            })))
            .mount(&upstream)
            .await;

        let port = portpicker::pick_unused_port().unwrap();
        let mut profile = GatewayProfile::new_default("replay".into(), "Replay".into(), port);
        profile.upstream_base_url = upstream.uri();
        let logs = LogStore::new(tempfile::tempdir().unwrap().path().into());
        let registry = GatewayRegistry::start(profile, logs, settings_store())
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let first = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-reasoner",
                "messages": [{ "role": "user", "content": "call a tool" }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(first.status(), reqwest::StatusCode::OK);

        let second = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-reasoner",
                "messages": [{
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }]
                }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::OK);
        registry.stop().await;
    }

    #[tokio::test]
    async fn replays_openai_reasoning_after_streamed_tool_call_response() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "call a tool" }],
                "reasoning_effort": "high",
                "stream": true
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(
                        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"streamed \"}}]}\n\n\
                         data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}]}\n\n\
                         data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_stream\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{}\"}}]}}]}\n\n\
                         data: [DONE]\n\n",
                    ),
            )
            .mount(&upstream)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "streamed thinking",
                    "tool_calls": [{
                        "id": "call_stream",
                        "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }]
                }],
                "reasoning_effort": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
            })))
            .mount(&upstream)
            .await;

        let port = portpicker::pick_unused_port().unwrap();
        let mut profile =
            GatewayProfile::new_default("stream-replay".into(), "Stream Replay".into(), port);
        profile.upstream_base_url = upstream.uri();
        let logs = LogStore::new(tempfile::tempdir().unwrap().path().into());
        let registry = GatewayRegistry::start(profile, logs, settings_store())
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let first = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-reasoner",
                "messages": [{ "role": "user", "content": "call a tool" }],
                "stream": true
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        let _ = first.text().await.unwrap();
        let cached = registry.reasoning_cache.lock().await.clone();
        assert_eq!(
            cached.get("call_stream").map(String::as_str),
            Some("streamed thinking")
        );

        let second = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-reasoner",
                "messages": [{
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_stream",
                        "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }]
                }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::OK);
        registry.stop().await;
    }

    #[tokio::test]
    async fn replays_openai_reasoning_before_stream_finish_when_client_stops_reading() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "call a tool" }],
                "reasoning_effort": "high",
                "stream": true
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(
                        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"early reasoning\"}}]}\n\n\
                         data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_early\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{}\"}}]}}]}\n\n",
                    ),
            )
            .mount(&upstream)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_json(json!({
                "model": "deepseek-v4-pro",
                "messages": [{
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "early reasoning",
                    "tool_calls": [{
                        "id": "call_early",
                        "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }]
                }],
                "reasoning_effort": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
            })))
            .mount(&upstream)
            .await;

        let port = portpicker::pick_unused_port().unwrap();
        let mut profile =
            GatewayProfile::new_default("early-replay".into(), "Early Replay".into(), port);
        profile.upstream_base_url = upstream.uri();
        let logs = LogStore::new(tempfile::tempdir().unwrap().path().into());
        let registry = GatewayRegistry::start(profile, logs, settings_store())
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let mut first = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-reasoner",
                "messages": [{ "role": "user", "content": "call a tool" }],
                "stream": true
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        let _ = first.chunk().await.unwrap();

        let second = client
            .post(format!("http://127.0.0.1:{port}/chat/completions"))
            .json(&json!({
                "model": "deepseek-reasoner",
                "messages": [{
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "",
                    "tool_calls": [{
                        "id": "call_early",
                        "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }]
                }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::OK);
        registry.stop().await;
    }

    #[tokio::test]
    async fn reports_port_conflicts() {
        let port = portpicker::pick_unused_port().unwrap();
        let _listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
            .await
            .unwrap();
        let profile = GatewayProfile::new_default("busy".into(), "Busy".into(), port);
        let logs = LogStore::new(tempfile::tempdir().unwrap().path().into());
        let error = match GatewayRegistry::start(profile, logs, settings_store()).await {
            Ok(registry) => {
                registry.stop().await;
                panic!("gateway unexpectedly started on a busy port");
            }
            Err(error) => error,
        };
        assert!(error.to_string().contains("not available"));
    }
}
