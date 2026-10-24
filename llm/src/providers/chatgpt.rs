use rig::tool::Tool;
use schemars::JsonSchema;
use serde_json::{json, Value};

use crate::{error::Error, providers::CompletionProvider, tools::ToolWrapper};

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

/// OpenAI's ChatGPT-plan provider.
///
/// Sign in with ChatGPT has a stricter request contract than API-key access:
/// requests must be streamed, must not be stored, and cannot include sampling
/// or output-token controls. Keep this provider separate from the regular
/// OpenAI provider so those constraints cannot accidentally regress.
pub struct ChatGPT {
    access_token: String,
    model: String,
    system_message: String,
    client: reqwest::Client,
}

impl ChatGPT {
    pub fn new<T: Tool + 'static>(
        access_token: &str,
        model: &str,
        system_message: Option<&str>,
        _tools: Vec<ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        if access_token.trim().is_empty() {
            return Err(Error::AuthenticationError(
                "access token is required for ChatGPT".to_string(),
            ));
        }

        Ok(Self {
            access_token: access_token.to_string(),
            model: model.to_string(),
            system_message: system_message.unwrap_or_default().to_string(),
            client: reqwest::Client::new(),
        })
    }

    async fn response_text(&self, prompt: &str, schema: Option<Value>) -> Result<String, Error> {
        let mut payload = json!({
            "model": self.model,
            "instructions": self.system_message,
            "input": [{
                "role": "user",
                "content": prompt,
            }],
            "store": false,
            "stream": true,
        });

        if let Some(schema) = schema {
            payload["text"] = json!({
                "format": {
                    "type": "json_schema",
                    "name": "riimail_email",
                    "strict": true,
                    "schema": schema,
                }
            });
        }

        let response = self
            .client
            .post(RESPONSES_URL)
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|error| Error::ProviderError(format!("ChatGPT request failed: {error}")))?;

        let status = response.status();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        let event_stream = response
            .text()
            .await
            .map_err(|error| Error::ProviderError(format!("ChatGPT stream failed: {error}")))?;

        if !status.is_success() {
            return Err(Error::ProviderError(format!(
                "ChatGPT returned HTTP {status} (request {request_id}): {event_stream}"
            )));
        }

        parse_event_stream(&event_stream, &request_id)
    }
}

#[async_trait::async_trait]
impl<J: JsonSchema + serde::de::DeserializeOwned> CompletionProvider<J> for ChatGPT {
    async fn chat(&self, prompt: &str) -> Result<String, Error> {
        self.response_text(prompt, None).await
    }

    async fn schema_chat(&self, prompt: &str) -> Result<J, Error> {
        let mut schema = serde_json::to_value(schemars::schema_for!(J))?;
        make_strict_json_schema(&mut schema);
        let response = self.response_text(prompt, Some(schema)).await?;
        Ok(serde_json::from_str(&response)?)
    }
}

fn make_strict_json_schema(schema: &mut Value) {
    match schema {
        Value::Object(object) => {
            let property_names = object
                .get("properties")
                .and_then(Value::as_object)
                .map(|properties| properties.keys().cloned().collect::<Vec<_>>());
            let has_object_type = match object.get("type") {
                Some(Value::String(kind)) => kind == "object",
                Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "object"),
                _ => false,
            };

            if has_object_type || property_names.is_some() {
                object.insert("additionalProperties".to_string(), Value::Bool(false));
                if let Some(property_names) = property_names {
                    object.insert(
                        "required".to_string(),
                        Value::Array(property_names.into_iter().map(Value::String).collect()),
                    );
                }
            }

            for value in object.values_mut() {
                make_strict_json_schema(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                make_strict_json_schema(value);
            }
        }
        _ => {}
    }
}

fn parse_event_stream(stream: &str, request_id: &str) -> Result<String, Error> {
    let mut output = String::new();
    let mut completed = false;

    for line in stream.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }

        let event: Value = serde_json::from_str(data)?;
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    output.push_str(delta);
                }
            }
            Some("response.completed") => completed = true,
            Some("response.failed") | Some("error") => {
                let error = event
                    .pointer("/response/error")
                    .or_else(|| event.get("error"))
                    .unwrap_or(&event);
                return Err(Error::ProviderError(format!(
                    "ChatGPT inference failed (request {request_id}): {error}"
                )));
            }
            _ => {}
        }
    }

    if !completed {
        return Err(Error::ProviderError(format!(
            "ChatGPT stream ended before response.completed (request {request_id})"
        )));
    }
    if output.is_empty() {
        return Err(Error::ProviderError(format!(
            "ChatGPT completed without output text (request {request_id})"
        )));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{make_strict_json_schema, parse_event_stream};

    #[test]
    fn normalizes_nested_objects_for_openai_strict_schema() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string" },
                "metadata": {
                    "type": "object",
                    "properties": {
                        "priority": { "type": "string" }
                    }
                }
            },
            "required": ["subject"]
        });

        make_strict_json_schema(&mut schema);

        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["metadata", "subject"]));
        assert_eq!(
            schema["properties"]["metadata"]["additionalProperties"],
            false
        );
        assert_eq!(
            schema["properties"]["metadata"]["required"],
            json!(["priority"])
        );
    }

    #[test]
    fn parses_completed_sse_response() {
        let stream = concat!(
            "event: response.output_text.delta\n",
            r#"data: {"type":"response.output_text.delta","delta":"{\"subject\":\""}"#,
            "\n\n",
            r#"data: {"type":"response.output_text.delta","delta":"Hello\"}"}"#,
            "\n\n",
            "data: {\"type\":\"response.completed\"}\n\n"
        );

        assert_eq!(
            parse_event_stream(stream, "req_test").expect("stream should parse"),
            "{\"subject\":\"Hello\"}"
        );
    }

    #[test]
    fn rejects_incomplete_sse_response() {
        let error = parse_event_stream(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n",
            "req_test",
        )
        .expect_err("incomplete stream should fail");

        assert!(error.to_string().contains("response.completed"));
    }
}
