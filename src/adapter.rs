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
        let mut raw_turns: Vec<(String, Vec<GeminiPart>)> = Vec::new();

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
                    let mut parts = Vec::new();
                    if !content_text.is_empty() {
                        parts.push(GeminiPart {
                            text: Some(content_text),
                            thought: None,
                            thought_signature: None,
                            function_call: None,
                            function_response: None,
                        });
                    }
                    if let Some(tool_calls) = &msg.tool_calls {
                        for tc in tool_calls {
                            let args_val = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                            parts.push(GeminiPart {
                                text: None,
                                thought: None,
                                thought_signature: None,
                                function_call: Some(GeminiFunctionCall {
                                    name: tc.function.name.clone(),
                                    args: args_val,
                                    id: Some(tc.id.clone()),
                                }),
                                function_response: None,
                            });
                        }
                    }
                    raw_turns.push(("model".to_string(), parts));
                }
                "tool" => {
                    let fn_name = msg.name.clone().unwrap_or_else(|| "function".to_string());
                    let resp_val = serde_json::from_str(&content_text)
                        .unwrap_or_else(|_| serde_json::json!({ "output": content_text }));
                    let part = GeminiPart {
                        text: None,
                        thought: None,
                        thought_signature: None,
                        function_call: None,
                        function_response: Some(GeminiFunctionResponse {
                            name: fn_name,
                            response: resp_val,
                        }),
                    };
                    raw_turns.push(("user".to_string(), vec![part]));
                }
                _ => {
                    // "user" and any custom roles
                    let part = GeminiPart {
                        text: Some(content_text),
                        thought: None,
                        thought_signature: None,
                        function_call: None,
                        function_response: None,
                    };
                    raw_turns.push(("user".to_string(), vec![part]));
                }
            }
        }

        let system_instruction = if !system_text.is_empty() {
            Some(GeminiSystemInstruction {
                parts: vec![GeminiPart {
                    text: Some(system_text),
                    thought: None,
                    thought_signature: None,
                    function_call: None,
                    function_response: None,
                }],
            })
        } else {
            None
        };

        // Normalize roles: merge consecutive turns of the same role
        let mut contents: Vec<GeminiContent> = Vec::new();
        for (role, parts) in raw_turns {
            if let Some(last) = contents.last_mut() {
                if last.role == role {
                    last.parts.extend(parts);
                    continue;
                }
            }
            contents.push(GeminiContent { role, parts });
        }

        // If contents is empty, ensure at least one empty user turn
        if contents.is_empty() {
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: Some("".to_string()),
                    thought: None,
                    thought_signature: None,
                    function_call: None,
                    function_response: None,
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

        let tools = req.tools.as_ref().map(|tool_list| {
            let mut decls = Vec::new();
            for t in tool_list {
                let mut decl = serde_json::json!({
                    "name": t.function.name,
                });
                if let Some(desc) = &t.function.description {
                    decl["description"] = serde_json::json!(desc);
                }
                if let Some(params) = &t.function.parameters {
                    decl["parameters"] = params.clone();
                }
                decls.push(decl);
            }
            vec![GeminiTool {
                function_declarations: decls,
            }]
        });

        let tool_config = req.tool_choice.as_ref().map(|tc| {
            let (mode, allowed) = match tc {
                serde_json::Value::String(s) if s == "auto" => ("AUTO".to_string(), None),
                serde_json::Value::String(s) if s == "required" => ("ANY".to_string(), None),
                serde_json::Value::String(s) if s == "none" => ("NONE".to_string(), None),
                serde_json::Value::String(name) => ("ANY".to_string(), Some(vec![name.clone()])),
                serde_json::Value::Object(obj) => {
                    if let Some(f) = obj
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        ("ANY".to_string(), Some(vec![f.to_string()]))
                    } else {
                        ("ANY".to_string(), None)
                    }
                }
                _ => ("AUTO".to_string(), None),
            };
            GeminiToolConfig {
                function_calling_config: Some(GeminiFunctionCallingConfig {
                    mode,
                    allowed_function_names: allowed,
                }),
            }
        });

        let request = GeminiRequest {
            system_instruction,
            contents,
            generation_config,
            tools,
            tool_config,
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
    ) -> (String, Vec<ToolCall>, Option<String>, Option<UsageInfo>) {
        let mut text = String::new();
        let mut tool_calls = Vec::new();
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
                                if let Some(fc) = &part.function_call {
                                    let call_id = fc
                                        .id
                                        .clone()
                                        .unwrap_or_else(|| format!("call_{}", uuid_simple()));
                                    tool_calls.push(ToolCall {
                                        id: call_id,
                                        r#type: "function".to_string(),
                                        function: FunctionCallInstance {
                                            name: fc.name.clone(),
                                            arguments: fc.args.to_string(),
                                        },
                                    });
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

        (text, tool_calls, finish_reason, usage)
    }

    /// Builds a completed OpenAI ChatCompletionResponse
    pub fn build_chat_response(
        model: String,
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
        finish_reason: Option<String>,
        usage: UsageInfo,
    ) -> ChatCompletionResponse {
        let now = Utc::now().timestamp();
        let final_finish_reason = if tool_calls.is_some() {
            Some("tool_calls".to_string())
        } else {
            finish_reason.or(Some("stop".to_string()))
        };

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
                    tool_calls,
                },
                finish_reason: final_finish_reason,
            }],
            usage,
        }
    }

    pub fn build_chat_chunk(
        id: &str,
        created: i64,
        model: &str,
        delta: ChatChunkDelta,
        finish_reason: Option<String>,
        usage: Option<UsageInfo>,
    ) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id.to_string(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model.to_string(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta,
                finish_reason,
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
                        function_call: None,
                        function_response: None,
                    }],
                }],
                generation_config: Some(GeminiGenerationConfig {
                    temperature,
                    max_output_tokens,
                    stop_sequences,
                    response_mime_type: None,
                }),
                tools: None,
                tool_config: None,
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
