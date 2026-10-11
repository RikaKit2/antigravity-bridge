use crate::adapter::Adapter;
use crate::auth::TokenManager;
use crate::config::AppConfig;
use crate::types::*;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct AppState {
    pub client: reqwest::Client,
    pub token_manager: TokenManager,
    pub config: Arc<RwLock<AppConfig>>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/v1/models", get(handle_models))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/completions", post(handle_fim_completions))
        .route("/completions", post(handle_fim_completions))
        .with_state(state)
}

async fn health_check() -> &'static str {
    "OK"
}

async fn handle_models(State(state): State<AppState>) -> Response {
    let config = state.config.read().await;
    let mut models = vec![
        config.routing.primary.clone(),
        "gemini-3.8-flash".to_string(),
        "gemini-3.6-flash".to_string(),
        "gemini-3.5-flash".to_string(),
        "gemini-3.5-flash-lite".to_string(),
        "gemini-3.1-pro".to_string(),
        "gemini-2.5-flash".to_string(),
        "gemini-2.5-pro".to_string(),
        "claude-sonnet-4-6".to_string(),
        "tab_flash_lite_preview".to_string(),
    ];
    models.sort();
    models.dedup();

    let now = Utc::now().timestamp();

    let data = models
        .into_iter()
        .map(|id| ModelEntry {
            id,
            object: "model".to_string(),
            created: now,
            owned_by: "google-antigravity".to_string(),
        })
        .collect();

    Json(ModelsList {
        object: "list".to_string(),
        data,
    })
    .into_response()
}

async fn handle_chat_completions(
    State(state): State<AppState>,
    Json(payload): Json<ChatCompletionRequest>,
) -> Response {
    let config = state.config.read().await.clone();
    let candidates = config.candidate_chain(payload.model.as_deref());
    let project_id = state.token_manager.get_project_id().await;

    let mut last_error = String::new();

    for (idx, candidate_model) in candidates.iter().enumerate() {
        let wire_model = config.resolve_model(candidate_model);
        debug!(
            "Attempting chat completion candidate {}/{}: {} (wire: {})",
            idx + 1,
            candidates.len(),
            candidate_model,
            wire_model
        );

        let envelope = Adapter::build_chat_envelope(&payload, &wire_model, project_id.clone());

        match send_envelope_with_retry(&state, &config.server.upstream_url, &envelope).await {
            Ok((raw_text, finish_reason, usage)) => {
                info!(
                    "Chat completion succeeded using model: {} (wire: {})",
                    candidate_model, wire_model
                );

                let is_json = payload
                    .response_format
                    .as_ref()
                    .and_then(|rf| rf.r#type.as_deref())
                    == Some("json_object");

                let content = if is_json {
                    Adapter::clean_json_response(&raw_text)
                } else {
                    raw_text
                };

                let response = Adapter::build_chat_response(
                    candidate_model.clone(),
                    content,
                    finish_reason,
                    usage.unwrap_or(UsageInfo {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    }),
                );

                return Json(response).into_response();
            }
            Err((status, err_msg, is_quota_or_capacity)) => {
                warn!(
                    "Model candidate '{}' failed with status {}: {}. Quota/capacity: {}",
                    candidate_model, status, err_msg, is_quota_or_capacity
                );
                last_error = err_msg;

                // If not quota/capacity and not last candidate, continue fallback
                if idx + 1 < candidates.len() {
                    info!(
                        "Falling back from '{}' to '{}'",
                        candidate_model,
                        candidates[idx + 1]
                    );
                    continue;
                }
            }
        }
    }

    error!("All candidate models exhausted: {}", last_error);
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": {
                "message": format!("All model candidates failed: {}", last_error),
                "type": "upstream_error"
            }
        })),
    )
        .into_response()
}

async fn handle_fim_completions(
    State(state): State<AppState>,
    Json(payload): Json<CompletionRequest>,
) -> Response {
    let config = state.config.read().await.clone();
    let default_model = "tab_flash_lite_preview";
    let project_id = state.token_manager.get_project_id().await;

    let (model, envelope, prompt_prefix) =
        Adapter::build_fim_envelope(&payload, default_model, project_id);

    match send_envelope_with_retry(&state, &config.server.upstream_url, &envelope).await {
        Ok((raw_text, finish_reason, usage)) => {
            let response = Adapter::build_fim_response(
                model,
                raw_text,
                &prompt_prefix,
                finish_reason,
                usage.unwrap_or(UsageInfo {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                }),
            );
            Json(response).into_response()
        }
        Err((status, err_msg, _)) => (
            status,
            Json(serde_json::json!({
                "error": {
                    "message": err_msg,
                    "type": "upstream_error"
                }
            })),
        )
            .into_response(),
    }
}

/// Sends envelope to Antigravity upstream with 401 retry and streams response
async fn send_envelope_with_retry(
    state: &AppState,
    upstream_url: &str,
    envelope: &AntigravityEnvelope,
) -> Result<(String, Option<String>, Option<UsageInfo>), (StatusCode, String, bool)> {
    for attempt in 0..2 {
        let token = state.token_manager.get_token().await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Token acquisition error: {}", e),
                false,
            )
        })?;

        let url = format!(
            "{}/v1internal:streamGenerateContent?alt=sse",
            upstream_url.trim_end_matches('/')
        );

        let send_res = state
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("User-Agent", "antigravity")
            .json(envelope)
            .send()
            .await;

        let response = match send_res {
            Ok(r) => r,
            Err(e) => {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to connect to upstream: {}", e),
                    false,
                ));
            }
        };

        let status = response.status();

        if status == reqwest::StatusCode::UNAUTHORIZED {
            state.token_manager.invalidate().await;
            if attempt == 0 {
                info!("Retrying request with refreshed token after 401");
                continue;
            }
            return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string(), false));
        }

        if !status.is_success() {
            let err_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            let is_quota_or_capacity = status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
                || err_body.contains("RESOURCE_EXHAUSTED")
                || err_body.contains("No capacity available")
                || err_body.contains("quota");

            let sc = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            return Err((sc, err_body, is_quota_or_capacity));
        }

        // Stream and accumulate SSE response
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut accumulated_text = String::new();
        let mut last_finish_reason = None;
        let mut final_usage = None;

        while let Some(chunk_res) = stream.next().await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(e) => {
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        format!("Error reading stream chunk: {}", e),
                        false,
                    ));
                }
            };

            let chunk_str = String::from_utf8_lossy(&chunk);
            buffer.push_str(&chunk_str);

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer.drain(..=line_end);

                if let Some(json_str) = line.strip_prefix("data:") {
                    let json_str = json_str.trim();
                    if json_str.is_empty() {
                        continue;
                    }

                    if let Ok(event) = serde_json::from_str::<GeminiSseEvent>(json_str) {
                        let (text, finish_reason, usage) = Adapter::extract_chat_chunk(&event);
                        accumulated_text.push_str(&text);
                        if finish_reason.is_some() {
                            last_finish_reason = finish_reason;
                        }
                        if usage.is_some() {
                            final_usage = usage;
                        }
                    }
                }
            }
        }

        return Ok((accumulated_text, last_finish_reason, final_usage));
    }

    Err((
        StatusCode::BAD_GATEWAY,
        "Failed after retry".to_string(),
        false,
    ))
}
