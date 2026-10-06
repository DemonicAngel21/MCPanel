//! AI provider implementations: Google Gemini REST API and OpenAI-compatible endpoint.

use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::ai::{
    AiClient, AiFunctionCall, AiLlmMessage, AiLlmRequest, AiLlmResponse, AiToolDefinition,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::time::Duration;

pub const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
pub const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
pub const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com/v1";
pub const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com";
pub const GROQ_API_BASE: &str = "https://api.groq.com/openai/v1";
pub const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
pub const MISTRAL_API_BASE: &str = "https://api.mistral.ai/v1";
pub const OLLAMA_DEFAULT_BASE: &str = "http://localhost:11434/v1";
pub const LMSTUDIO_DEFAULT_BASE: &str = "http://localhost:1234/v1";

/// Resolve the default base URL for well-known AI providers.
pub fn default_base_url_for_provider(provider: &str) -> Option<&'static str> {
    match provider.to_ascii_lowercase().as_str() {
        "openai" => Some(OPENAI_API_BASE),
        "anthropic" | "claude" => Some(ANTHROPIC_API_BASE),
        "deepseek" => Some(DEEPSEEK_API_BASE),
        "groq" => Some(GROQ_API_BASE),
        "openrouter" => Some(OPENROUTER_API_BASE),
        "mistral" => Some(MISTRAL_API_BASE),
        "ollama" => Some(OLLAMA_DEFAULT_BASE),
        "lmstudio" => Some(LMSTUDIO_DEFAULT_BASE),
        "local" | "localai" | "vllm" => Some("http://localhost:8080/v1"),
        _ => None,
    }
}

pub struct GeminiAiClient {
    client: reqwest::Client,
}

impl GeminiAiClient {
    pub fn new(http: &HttpClient) -> Self {
        Self {
            client: http.inner().clone(),
        }
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn sanitize_error(e: reqwest::Error, provider: &str) -> CoreError {
        CoreError::new(
            ErrorCode::ProviderError,
            format!("AI provider ({provider}) request failed: {e}"),
        )
    }

    fn parse_provider_error(status: reqwest::StatusCode, body: &str, provider: &str) -> CoreError {
        if let Ok(val) = serde_json::from_str::<Value>(body)
            && let Some(msg) = val
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
        {
            return CoreError::new(ErrorCode::ProviderError, format!("AI ({provider}): {msg}"));
        }
        CoreError::new(
            ErrorCode::ProviderError,
            format!("AI ({provider}) returned error status {status}"),
        )
    }

    fn convert_gemini_tools(tools: &[AiToolDefinition]) -> Vec<Value> {
        let mut declarations = Vec::new();
        for t in tools {
            let mut props_map = serde_json::Map::new();
            for (pname, prop) in &t.parameters.properties {
                let ptype = prop.r#type.to_uppercase();
                let mut prop_obj = json!({
                    "type": ptype,
                    "description": prop.description,
                });
                if let Some(items) = &prop.items {
                    prop_obj["items"] = json!({
                        "type": items.r#type.to_uppercase(),
                        "description": items.description,
                    });
                }
                props_map.insert(pname.clone(), prop_obj);
            }

            declarations.push(json!({
                "name": t.name,
                "description": t.description,
                "parameters": {
                    "type": "OBJECT",
                    "properties": props_map,
                    "required": t.parameters.required,
                }
            }));
        }

        vec![json!({
            "functionDeclarations": declarations,
        })]
    }

    fn convert_gemini_contents(messages: &[AiLlmMessage]) -> Vec<Value> {
        let mut contents = Vec::new();
        for m in messages {
            let mut parts = Vec::new();
            if let Some(txt) = &m.content
                && !txt.is_empty()
            {
                parts.push(json!({ "text": txt }));
            }

            if let Some(calls) = &m.function_calls {
                for c in calls {
                    parts.push(json!({
                        "functionCall": {
                            "name": c.name,
                            "args": c.args,
                        }
                    }));
                }
            }

            if let Some(responses) = &m.function_responses {
                for r in responses {
                    let resp_obj = if r.response.is_object() {
                        r.response.clone()
                    } else {
                        json!({ "result": r.response })
                    };
                    parts.push(json!({
                        "functionResponse": {
                            "name": r.name,
                            "response": resp_obj,
                        }
                    }));
                }
            }

            if !parts.is_empty() {
                let role = match m.role.as_str() {
                    "model" | "assistant" => "model",
                    _ => "user",
                };
                contents.push(json!({
                    "role": role,
                    "parts": parts,
                }));
            }
        }
        contents
    }

    async fn gemini_complete(
        &self,
        key: &SecretString,
        model: &str,
        request: &AiLlmRequest,
    ) -> CoreResult<AiLlmResponse> {
        let clean_model = match model.strip_prefix("models/").unwrap_or(model) {
            "gemini-2.5-flash" => "gemini-3.8-flash",
            other => other,
        };
        let url = format!("{GEMINI_API_BASE}/models/{clean_model}:generateContent");
        let mut payload = json!({});

        if let Some(sys) = &request.system_instruction {
            payload["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        payload["contents"] = Value::Array(Self::convert_gemini_contents(&request.messages));

        if !request.tools.is_empty() {
            payload["tools"] = Value::Array(Self::convert_gemini_tools(&request.tools));
        }

        let resp = self
            .client
            .post(&url)
            .header("x-goog-api-key", key.expose_secret().trim())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(Duration::from_secs(60))
            .json(&payload)
            .send()
            .await
            .map_err(|e| Self::sanitize_error(e, "gemini"))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Self::sanitize_error(e, "gemini"))?;

        if !status.is_success() {
            return Err(Self::parse_provider_error(status, &body, "gemini"));
        }

        let resp_json: Value = serde_json::from_str(&body).map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("Invalid JSON response: {e}"),
            )
        })?;

        let mut text_parts = Vec::new();
        let mut function_calls = Vec::new();

        if let Some(parts) = resp_json
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|cand| cand.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            for p in parts {
                if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(t.to_string());
                }
                if let Some(fc) = p.get("functionCall") {
                    let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let args = fc.get("args").cloned().unwrap_or(json!({}));
                    function_calls.push(AiFunctionCall {
                        id: format!("call_{}", uuid::Uuid::new_v4().simple()),
                        name: name.to_string(),
                        args,
                    });
                }
            }
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };

        Ok(AiLlmResponse {
            content,
            function_calls,
        })
    }

    async fn openai_complete(
        &self,
        key: &SecretString,
        provider: &str,
        model: &str,
        base_url: Option<&str>,
        request: &AiLlmRequest,
    ) -> CoreResult<AiLlmResponse> {
        let default_base = default_base_url_for_provider(provider).unwrap_or(OPENAI_API_BASE);
        let base = base_url.unwrap_or(default_base).trim_end_matches('/');
        let url = format!("{base}/chat/completions");

        let mut messages = Vec::new();
        if let Some(sys) = &request.system_instruction {
            messages.push(json!({
                "role": "system",
                "content": sys,
            }));
        }

        for m in &request.messages {
            match m.role.as_str() {
                "function" => {
                    if let Some(resps) = &m.function_responses {
                        for r in resps {
                            messages.push(json!({
                                "role": "tool",
                                "tool_call_id": r.id,
                                "content": r.response.to_string(),
                            }));
                        }
                    }
                }
                "model" | "assistant" => {
                    let mut obj = json!({
                        "role": "assistant",
                    });
                    if let Some(c) = &m.content {
                        obj["content"] = json!(c);
                    }
                    if let Some(calls) = &m.function_calls {
                        let tool_calls: Vec<Value> = calls
                            .iter()
                            .map(|c| {
                                json!({
                                    "id": c.id,
                                    "type": "function",
                                    "function": {
                                        "name": c.name,
                                        "arguments": c.args.to_string(),
                                    }
                                })
                            })
                            .collect();
                        obj["tool_calls"] = json!(tool_calls);
                    }
                    messages.push(obj);
                }
                _ => {
                    messages.push(json!({
                        "role": "user",
                        "content": m.content.as_deref().unwrap_or(""),
                    }));
                }
            }
        }

        let mut payload = json!({
            "model": model,
            "messages": messages,
            "stream": false,
        });

        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": {
                                "type": "object",
                                "properties": t.parameters.properties,
                                "required": t.parameters.required,
                            }
                        }
                    })
                })
                .collect();
            payload["tools"] = json!(tools);
        }

        let mut req_builder = self
            .client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(Duration::from_secs(60));

        let key_str = key.expose_secret().trim();
        if !key_str.is_empty() && key_str != "local" {
            req_builder =
                req_builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {key_str}"));
        }

        if provider.eq_ignore_ascii_case("openrouter") {
            req_builder = req_builder
                .header("HTTP-Referer", "https://mcpanel.app")
                .header("X-Title", "MCPanel");
        }

        let resp = req_builder
            .json(&payload)
            .send()
            .await
            .map_err(|e| Self::sanitize_error(e, provider))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Self::sanitize_error(e, provider))?;

        if !status.is_success() {
            return Err(Self::parse_provider_error(status, &body, provider));
        }

        let resp_json: Value = serde_json::from_str(&body).map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("Invalid JSON response: {e}"),
            )
        })?;

        let choice = resp_json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"));

        let content = choice
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        let mut function_calls = Vec::new();
        if let Some(tools) = choice
            .and_then(|m| m.get("tool_calls"))
            .and_then(|t| t.as_array())
        {
            for tc in tools {
                let id = tc
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some(func) = tc.get("function") {
                    let name = func
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let raw_args = func
                        .get("arguments")
                        .and_then(|a| a.as_str())
                        .unwrap_or("{}");
                    let args = serde_json::from_str(raw_args).unwrap_or(json!({}));
                    function_calls.push(AiFunctionCall { id, name, args });
                }
            }
        }

        Ok(AiLlmResponse {
            content,
            function_calls,
        })
    }

    async fn anthropic_complete(
        &self,
        key: &SecretString,
        model: &str,
        base_url: Option<&str>,
        request: &AiLlmRequest,
    ) -> CoreResult<AiLlmResponse> {
        let base = base_url.unwrap_or(ANTHROPIC_API_BASE).trim_end_matches('/');
        let url = format!("{base}/messages");

        let mut messages: Vec<Value> = Vec::new();
        for m in &request.messages {
            match m.role.as_str() {
                "function" => {
                    if let Some(resps) = &m.function_responses {
                        let mut content_parts = Vec::new();
                        for r in resps {
                            content_parts.push(json!({
                                "type": "tool_result",
                                "tool_use_id": r.id,
                                "content": r.response.to_string(),
                            }));
                        }
                        if !content_parts.is_empty() {
                            messages.push(json!({
                                "role": "user",
                                "content": content_parts,
                            }));
                        }
                    }
                }
                "model" | "assistant" => {
                    let mut content_parts = Vec::new();
                    if let Some(txt) = &m.content
                        && !txt.is_empty()
                    {
                        content_parts.push(json!({
                            "type": "text",
                            "text": txt,
                        }));
                    }
                    if let Some(calls) = &m.function_calls {
                        for c in calls {
                            content_parts.push(json!({
                                "type": "tool_use",
                                "id": c.id,
                                "name": c.name,
                                "input": c.args,
                            }));
                        }
                    }
                    if !content_parts.is_empty() {
                        messages.push(json!({
                            "role": "assistant",
                            "content": content_parts,
                        }));
                    }
                }
                _ => {
                    if let Some(txt) = &m.content
                        && !txt.is_empty()
                    {
                        messages.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "text",
                                "text": txt,
                            }],
                        }));
                    }
                }
            }
        }

        if messages.is_empty() {
            messages.push(json!({
                "role": "user",
                "content": [{ "type": "text", "text": "ping" }]
            }));
        }

        let mut payload = json!({
            "model": model,
            "max_tokens": 4096,
            "messages": messages,
        });

        if let Some(sys) = &request.system_instruction {
            payload["system"] = json!(sys);
        }

        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": {
                            "type": "object",
                            "properties": t.parameters.properties,
                            "required": t.parameters.required,
                        }
                    })
                })
                .collect();
            payload["tools"] = json!(tools);
        }

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", key.expose_secret().trim())
            .header("anthropic-version", "2023-06-01")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(Duration::from_secs(60))
            .json(&payload)
            .send()
            .await
            .map_err(|e| Self::sanitize_error(e, "anthropic"))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Self::sanitize_error(e, "anthropic"))?;

        if !status.is_success() {
            return Err(Self::parse_provider_error(status, &body, "anthropic"));
        }

        let resp_json: Value = serde_json::from_str(&body).map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("Invalid JSON response: {e}"),
            )
        })?;

        let mut text_parts = Vec::new();
        let mut function_calls = Vec::new();

        if let Some(parts) = resp_json.get("content").and_then(|c| c.as_array()) {
            for p in parts {
                let item_type = p.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if item_type == "text" {
                    if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(t.to_string());
                    }
                } else if item_type == "tool_use" {
                    let id = p
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = p
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = p.get("input").cloned().unwrap_or(json!({}));
                    function_calls.push(AiFunctionCall { id, name, args });
                }
            }
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };

        Ok(AiLlmResponse {
            content,
            function_calls,
        })
    }
}

#[async_trait]
impl AiClient for GeminiAiClient {
    async fn test_key(
        &self,
        key: &SecretString,
        provider: &str,
        model: &str,
        base_url: Option<&str>,
    ) -> CoreResult<()> {
        match provider.to_ascii_lowercase().as_str() {
            "gemini" => {
                let clean_model = match model.strip_prefix("models/").unwrap_or(model) {
                    "gemini-2.5-flash" => "gemini-3.8-flash",
                    other => other,
                };
                let url = format!("{GEMINI_API_BASE}/models/{clean_model}:generateContent");
                let payload = json!({
                    "contents": [{
                        "parts": [{ "text": "ping" }]
                    }]
                });

                let resp = self
                    .client
                    .post(&url)
                    .header("x-goog-api-key", key.expose_secret().trim())
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .timeout(Duration::from_secs(15))
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| Self::sanitize_error(e, "gemini"))?;

                let status = resp.status();
                if status.is_success() {
                    Ok(())
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    Err(Self::parse_provider_error(status, &body, "gemini"))
                }
            }
            "anthropic" | "claude" => {
                let req = AiLlmRequest {
                    system_instruction: None,
                    messages: vec![AiLlmMessage {
                        role: "user".into(),
                        content: Some("ping".into()),
                        function_calls: None,
                        function_responses: None,
                    }],
                    tools: vec![],
                };
                let _ = self.anthropic_complete(key, model, base_url, &req).await?;
                Ok(())
            }
            _ => {
                // OpenAI, DeepSeek, Groq, OpenRouter, Mistral, Ollama, LM Studio, Custom
                let req = AiLlmRequest {
                    system_instruction: None,
                    messages: vec![AiLlmMessage {
                        role: "user".into(),
                        content: Some("ping".into()),
                        function_calls: None,
                        function_responses: None,
                    }],
                    tools: vec![],
                };
                let _ = self
                    .openai_complete(key, provider, model, base_url, &req)
                    .await?;
                Ok(())
            }
        }
    }

    async fn complete_turn(
        &self,
        key: &SecretString,
        provider: &str,
        model: &str,
        base_url: Option<&str>,
        request: &AiLlmRequest,
    ) -> CoreResult<AiLlmResponse> {
        match provider.to_ascii_lowercase().as_str() {
            "gemini" => self.gemini_complete(key, model, request).await,
            "anthropic" | "claude" => self.anthropic_complete(key, model, base_url, request).await,
            _ => {
                self.openai_complete(key, provider, model, base_url, request)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_base_urls() {
        assert_eq!(
            default_base_url_for_provider("openai"),
            Some(OPENAI_API_BASE)
        );
        assert_eq!(
            default_base_url_for_provider("anthropic"),
            Some(ANTHROPIC_API_BASE)
        );
        assert_eq!(
            default_base_url_for_provider("claude"),
            Some(ANTHROPIC_API_BASE)
        );
        assert_eq!(
            default_base_url_for_provider("deepseek"),
            Some(DEEPSEEK_API_BASE)
        );
        assert_eq!(default_base_url_for_provider("groq"), Some(GROQ_API_BASE));
        assert_eq!(
            default_base_url_for_provider("openrouter"),
            Some(OPENROUTER_API_BASE)
        );
        assert_eq!(
            default_base_url_for_provider("mistral"),
            Some(MISTRAL_API_BASE)
        );
        assert_eq!(
            default_base_url_for_provider("ollama"),
            Some(OLLAMA_DEFAULT_BASE)
        );
        assert_eq!(
            default_base_url_for_provider("lmstudio"),
            Some(LMSTUDIO_DEFAULT_BASE)
        );
        assert_eq!(default_base_url_for_provider("unknown_provider"), None);
    }
}
