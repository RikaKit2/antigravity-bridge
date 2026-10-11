use crate::types::*;
use chrono::Utc;

pub struct Adapter;

impl Adapter {
    /// Translates an OpenAI ChatCompletionRequest into a Gemini AntigravityEnvelope
    pub fn build_chat_envelope(
        req: &ChatCompletionRequest,
        target_model: &str,
        project_id: String,
    ) -> AntigravityEnvelope {
        let mut system_text = String::new();
        let mut raw_turns: Vec<(String, String)> = Vec::new();

        for msg in &req.messages {
            let content_text = msg
                .content
                .as_ref()
                .map(|c| c.to_plain_text())
                .unwrap_or_default();

            match msg.role.as_str() {
                "system" => {
                    if !system_text.is_empty() {
                        system_text.push_str("\n\n");
                    }
                    system_text.push_str(&content_text);
                }
                "assistant" => {
                    raw_turns.push(("model".to_string(), content_text));
                }
                _ => {
                    // "user" and any custom roles
                    raw_turns.push(("user".to_string(), content_text));
                }
            }
        }

        let system_instruction = if !system_text.is_empty() {
            Some(GeminiSystemInstruction {
                parts: vec![GeminiPart {
                    text: Some(system_text),
                    thought: None,
                    thought_signature: None,
                }],
            })
        } else {
            None
        };

        // Normalize roles: merge consecutive turns of the same role
        let mut contents: Vec<GeminiContent> = Vec::new();
        for (role, text) in raw_turns {
            if let Some(last) = contents.last_mut() {
                if last.role == role {
                    last.parts.push(GeminiPart {
                        text: Some(text),
                        thought: None,
                        thought_signature: None,
                    });
                    continue;
                }
            }
            contents.push(GeminiContent {
                role,
                parts: vec![GeminiPart {
                    text: Some(text),
                    thought: None,
                    thought_signature: None,
                }],
            });
        }

        // If contents is empty, ensure at least one empty user turn
        if contents.is_empty() {
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: Some("".to_string()),
                    thought: None,
                    thought_signature: None,
                }],
            });
        }

        // Generation config
        let response_mime_type = req.response_format.as_ref().and_then(|rf| {
            if rf.r#type.as_deref() == Some("json_object") {
                Some("application/json".to_string())
            } else {
                None
            }
        });

        let stop_sequences = req.stop.as_ref().map(|s| s.to_vec());
        let max_output_tokens = req.max_tokens.or(req.max_completion_tokens);

        let generation_config = Some(GeminiGenerationConfig {
            temperature: req.temperature,
            max_output_tokens,
            stop_sequences,
            response_mime_type,
        });

        let request = GeminiRequest {
            system_instruction,
            contents,
            generation_config,
        };

        AntigravityEnvelope {
            project: project_id,
            model: target_model.to_string(),
            request,
            request_type: Some("agent".to_string()),
            request_id: Some(format!("agent-{}", uuid_simple())),
            user_agent: Some("antigravity".to_string()),
        }
    }

    /// Strips markdown code block formatting (```json ... ```) when JSON is expected
    pub fn clean_json_response(raw: &str) -> String {
        let trimmed = raw.trim();
        if let Some(stripped) = trimmed.strip_prefix("```json") {
            if let Some(inner) = stripped.strip_suffix("```") {
                return inner.trim().to_string();
            }
        }
        if let Some(stripped) = trimmed.strip_prefix("```") {
            if let Some(inner) = stripped.strip_suffix("```") {
                return inner.trim().to_string();
            }
        }
        trimmed.to_string()
    }

    /// Parses SSE line and extracts non-thought text and usage metadata
    pub fn extract_chat_chunk(
        event: &GeminiSseEvent,
    ) -> (String, Option<String>, Option<UsageInfo>) {
        let mut text = String::new();
        let mut finish_reason = None;
        let mut usage = None;

        if let Some(inner) = &event.response {
            if let Some(candidates) = &inner.candidates {
                if let Some(cand) = candidates.first() {
                    finish_reason = cand.finish_reason.clone();
                    if let Some(content) = &cand.content {
                        if let Some(parts) = &content.parts {
                            for part in parts {
                                // Skip thinking/reasoning parts
                                if part.thought == Some(true) {
                                    continue;
                                }
                                if let Some(t) = &part.text {
                                    text.push_str(t);
                                }
                            }
                        }
                    }
                }
            }

            if let Some(meta) = &inner.usage_metadata {
                usage = Some(UsageInfo {
                    prompt_tokens: meta.prompt_token_count.unwrap_or(0),
                    completion_tokens: meta.candidates_token_count.unwrap_or(0),
                    total_tokens: meta.total_token_count.unwrap_or(0),
                });
            }
        }

        (text, finish_reason, usage)
    }

    /// Builds a completed OpenAI ChatCompletionResponse
    pub fn build_chat_response(
        model: String,
        content: String,
        finish_reason: Option<String>,
        usage: UsageInfo,
    ) -> ChatCompletionResponse {
        let now = Utc::now().timestamp();

        ChatCompletionResponse {
            id: format!("chatcmpl-{}", uuid_simple()),
            object: "chat.completion".to_string(),
            created: now,
            model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatResponseMessage {
                    role: "assistant".to_string(),
                    content,
                },
                finish_reason: finish_reason.or(Some("stop".to_string())),
            }],
            usage,
        }
    }

    // --- Legacy FIM completion for Zed IDE ---

    pub fn build_fim_envelope(
        req: &CompletionRequest,
        default_model: &str,
        project_id: String,
    ) -> (String, AntigravityEnvelope, String) {
        let model = req.model.as_deref().unwrap_or(default_model).to_string();

        let prompt = req.prompt.as_ref().map(|p| p.as_str()).unwrap_or("");
        let prompt_prefix = Self::extract_prompt_prefix(prompt);

        let stop_sequences = req.stop.as_ref().map(|s| s.to_vec());
        let max_output_tokens = req.max_tokens.or(Some(64));
        let temperature = req.temperature.or(Some(0.1));

        let envelope = AntigravityEnvelope {
            project: project_id,
            model: model.clone(),
            request: GeminiRequest {
                system_instruction: None,
                contents: vec![GeminiContent {
                    role: "user".to_string(),
                    parts: vec![GeminiPart {
                        text: Some(prompt.to_string()),
                        thought: None,
                        thought_signature: None,
                    }],
                }],
                generation_config: Some(GeminiGenerationConfig {
                    temperature,
                    max_output_tokens,
                    stop_sequences,
                    response_mime_type: None,
                }),
            },
            request_type: Some("agent".to_string()),
            request_id: Some(format!("agent-{}", uuid_simple())),
            user_agent: Some("antigravity".to_string()),
        };

        (model, envelope, prompt_prefix)
    }

    pub fn build_fim_response(
        model: String,
        raw_text: String,
        prompt_prefix: &str,
        finish_reason: Option<String>,
        usage: UsageInfo,
    ) -> CompletionResponse {
        let cleaned = Self::clean_fim_completion(&raw_text, prompt_prefix);
        let now = Utc::now().timestamp();

        CompletionResponse {
            id: format!("cmpl-{}", uuid_simple()),
            object: "text_completion".to_string(),
            created: now,
            model,
            choices: vec![CompletionChoice {
                text: cleaned,
                index: 0,
                logprobs: None,
                finish_reason: finish_reason.or(Some("stop".to_string())),
            }],
            usage,
        }
    }

    fn extract_prompt_prefix(prompt: &str) -> String {
        const PREFIX_MARKER: &str = "<|fim_prefix|>";
        const SUFFIX_MARKER: &str = "<|fim_suffix|>";

        if let Some(prefix_start) = prompt.find(PREFIX_MARKER) {
            let after_prefix = &prompt[prefix_start + PREFIX_MARKER.len()..];
            if let Some(suffix_start) = after_prefix.find(SUFFIX_MARKER) {
                return after_prefix[..suffix_start].to_string();
            }
            return after_prefix.to_string();
        }
        prompt.to_string()
    }

    fn clean_fim_completion(generated: &str, prompt_prefix: &str) -> String {
        let mut text = generated;
        const MIDDLE_MARKER: &str = "<|fim_middle|>";
        const EOT_MARKER: &str = "<|file_separator|>";

        if let Some(pos) = text.find(MIDDLE_MARKER) {
            text = &text[pos + MIDDLE_MARKER.len()..];
        }
        if let Some(pos) = text.find(EOT_MARKER) {
            text = &text[..pos];
        }

        let mut cleaned = text.trim_start().to_string();
        if !prompt_prefix.is_empty() {
            let prefix_last_line = prompt_prefix.lines().last().unwrap_or("").trim_start();
            if !prefix_last_line.is_empty() && cleaned.starts_with(prefix_last_line) {
                cleaned = cleaned[prefix_last_line.len()..].to_string();
            }
        }
        cleaned
    }
}

fn uuid_simple() -> String {
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("{:x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_json_response() {
        let wrapped = "```json\n{\"fact\": \"hello\"}\n```";
        assert_eq!(
            Adapter::clean_json_response(wrapped),
            "{\"fact\": \"hello\"}"
        );

        let bare = "{\"fact\": \"hello\"}";
        assert_eq!(Adapter::clean_json_response(bare), "{\"fact\": \"hello\"}");
    }
}
