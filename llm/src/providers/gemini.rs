use rig::tool::Tool;
use schemars::JsonSchema;
use serde_json::{json, Value};

use crate::{error::Error, providers::CompletionProvider, tools::ToolWrapper};

const API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

pub struct Gemini {
    api_key: String,
    model: String,
    system_message: String,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    client: reqwest::Client,
}

impl Gemini {
    pub fn new<T: Tool + 'static>(
        api_key: &str,
        model: &str,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        _tools: Vec<ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        if api_key.trim().is_empty() {
            return Err(Error::AuthenticationError(
                "API key is required for Gemini".to_string(),
            ));
        }
        if model.trim().is_empty() {
            return Err(Error::Other("model is required for Gemini".to_string()));
        }

        Ok(Self {
            api_key: api_key.trim().to_string(),
            model: model.to_string(),
            system_message: system_message.unwrap_or_default().to_string(),
            temperature,
            max_tokens,
            client: reqwest::Client::new(),
        })
    }

    async fn generate(
        &self,
        prompt: &str,
        context: Option<&str>,
        schema: Option<Value>,
    ) -> Result<String, Error> {
        let user_text = match context.filter(|value| !value.trim().is_empty()) {
            Some(context) => format!("Context:\n{context}\n\nRequest:\n{prompt}"),
            None => prompt.to_string(),
        };
        let mut generation_config = json!({});
        if let Some(temperature) = self.temperature {
            generation_config["temperature"] = json!(temperature);
        }
        if let Some(max_tokens) = self.max_tokens.filter(|value| *value > 0) {
            generation_config["maxOutputTokens"] = json!(max_tokens);
        }
        if let Some(schema) = schema {
            generation_config["responseMimeType"] = json!("application/json");
            generation_config["responseJsonSchema"] = schema;
        }

        let mut payload = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": user_text }]
            }],
            "generationConfig": generation_config,
        });
        if !self.system_message.trim().is_empty() {
            payload["systemInstruction"] = json!({
                "parts": [{ "text": self.system_message }]
            });
        }

        let url = format!("{API_BASE_URL}/{}:generateContent", self.model);
        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| Error::ProviderError(format!("Gemini request failed: {error}")))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|error| Error::ProviderError(format!("Invalid Gemini response: {error}")))?;

        if !status.is_success() {
            let message = body
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Unknown Gemini API error");
            return Err(Error::ProviderError(format!(
                "Gemini returned HTTP {status}: {message}"
            )));
        }

        extract_response_text(&body)
    }
}

#[async_trait::async_trait]
impl<J: JsonSchema + serde::de::DeserializeOwned> CompletionProvider<J> for Gemini {
    async fn chat(&self, prompt: &str, context: Option<&str>) -> Result<String, Error> {
        self.generate(prompt, context, None).await
    }

    async fn schema_chat(&self, prompt: &str, context: Option<&str>) -> Result<J, Error> {
        let mut schema = serde_json::to_value(schemars::schema_for!(J))?;
        remove_schema_metadata(&mut schema);
        let response = self.generate(prompt, context, Some(schema)).await?;
        Ok(serde_json::from_str(&response)?)
    }
}

fn extract_response_text(body: &Value) -> Result<String, Error> {
    let text = body
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .filter(|text| !text.is_empty());

    text.ok_or_else(|| {
        let reason = body
            .pointer("/candidates/0/finishReason")
            .and_then(Value::as_str)
            .or_else(|| {
                body.pointer("/promptFeedback/blockReason")
                    .and_then(Value::as_str)
            })
            .unwrap_or("no text was returned");
        Error::ProviderError(format!("Gemini completed without output text: {reason}"))
    })
}

fn remove_schema_metadata(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("$schema");
            for child in object.values_mut() {
                remove_schema_metadata(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                remove_schema_metadata(child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{extract_response_text, remove_schema_metadata};

    #[test]
    fn extracts_text_from_all_returned_parts() {
        let response = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Hello" }, { "text": " world" }]
                }
            }]
        });

        assert_eq!(extract_response_text(&response).unwrap(), "Hello world");
    }

    #[test]
    fn reports_blocked_responses() {
        let response = json!({ "promptFeedback": { "blockReason": "SAFETY" } });
        let error = extract_response_text(&response).unwrap_err().to_string();

        assert!(error.contains("SAFETY"));
    }

    #[test]
    fn removes_unsupported_schema_dialect_metadata() {
        let mut schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "nested": {
                    "$schema": "ignored",
                    "type": "string"
                }
            }
        });

        remove_schema_metadata(&mut schema);

        assert!(schema.get("$schema").is_none());
        assert!(schema["properties"]["nested"].get("$schema").is_none());
    }
}
