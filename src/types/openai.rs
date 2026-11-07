#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub max_completion_tokens: Option<u32>,
    pub stop: Option<StopValue>,
    pub response_format: Option<ResponseFormat>,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<serde_json::Value>,
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<MessageContent>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Array(Vec<ContentPart>),
}

impl MessageContent {
    pub fn to_plain_text(&self) -> String {
        match self {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Array(parts) => {
                let mut text = String::new();
                for p in parts {
                    if let Some(t) = &p.text {
                        text.push_str(t);
                    }
                }
                text
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContentPart {
    pub r#type: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseFormat {
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StopValue {
    Single(String),
    Multiple(Vec<String>),
}

impl StopValue {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            StopValue::Single(s) => vec![s.clone()],
            StopValue::Multiple(v) => v.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: UsageInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub r#type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCallInstance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallInstance {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponseMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelsList {
    pub object: String,
    pub data: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

// Legacy text completion (Zed IDE FIM)
#[derive(Debug, Clone, Deserialize)]
pub struct CompletionRequest {
    pub model: Option<String>,
    pub prompt: Option<PromptValue>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop: Option<StopValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PromptValue {
    Single(String),
    Multiple(Vec<String>),
}

impl PromptValue {
    pub fn as_str(&self) -> &str {
        match self {
            PromptValue::Single(s) => s.as_str(),
            PromptValue::Multiple(v) => v.first().map(|s| s.as_str()).unwrap_or(""),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<CompletionChoice>,
    pub usage: UsageInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionChoice {
    pub text: String,
    pub index: usize,
    pub logprobs: Option<()>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChunkChoice {
    pub index: usize,
    pub delta: ChatChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ChatChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}
