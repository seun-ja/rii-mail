use std::fmt::Display;

use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

use crate::{
    error::Error,
    llm::{
        providers::{local_inference, ollama, openai, sagemaker},
        rpc::MessageType,
        tools::_ToolWrapper,
    },
};

mod providers;
mod rpc;
mod tools;

pub use rpc::{caller, AgentWorkerClient};

// #[cfg(test)]
// pub use providers::local_inference::LocalInferenceAI;

pub struct _LlmProvider {
    providers: Box<dyn _CompletionProvider>,
}

impl _LlmProvider {
    #[tracing::instrument(name = "agent.message", skip(self, email))]
    pub async fn _chat(&self, email: EmailRequest) -> Result<String, Error> {
        self.providers.chat(&email.to_string()).await
    }
}

#[async_trait::async_trait]
pub trait _CompletionProvider: Send + Sync {
    /// Returns a chat response for the given prompt.
    async fn chat(&self, prompt: &str) -> Result<String, Error>;
}

/// Supported AI providers for the agent server.
pub enum _Providers {
    /// Ollama provider: local AI model inference.
    Ollama,
    /// OpenAI provider: cloud-based AI model inference.
    OpenAI,
    /// Custom SageMaker AI provider: cloud-based AI model inference using SageMaker.
    ///
    /// The following environment variables must be set:
    /// - `AWS_ACCESS_KEY_ID`: the AWS access key ID to use.
    /// - `AWS_SECRET_ACCESS_KEY`: the AWS secret access key to use.
    /// - `AWS_REGION`: the AWS region to use.
    CustomSageMakerAI,
    LocalInference,
}

impl Display for _Providers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            _Providers::Ollama => write!(f, "ollama"),
            _Providers::OpenAI => write!(f, "openai"),
            _Providers::CustomSageMakerAI => write!(f, "sagemaker"),
            _Providers::LocalInference => write!(f, "local"),
        }
    }
}

impl From<&str> for _Providers {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "ollama" => _Providers::Ollama,
            "openai" => _Providers::OpenAI,
            "sagemaker" => _Providers::CustomSageMakerAI,
            "local" => _Providers::LocalInference,
            _ => panic!(
                "unknown provider: {}. Currently supported providers are: ollama, openai, sagemaker, local",
                value
            ),
        }
    }
}

impl _Providers {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn _init<T: Tool + 'static>(
        provider: _Providers,
        model: &str,
        api_key: Option<&str>,
        system_message: String,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<_ToolWrapper<T>>,
        script_name: Option<String>,
        function_handler: Option<String>,
    ) -> Result<Box<dyn _CompletionProvider>, Error> {
        let client: Box<dyn _CompletionProvider> = match provider {
            _Providers::Ollama => {
                let client = ollama::_OllamaAI::_new(
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Box::new(client)
            }
            _Providers::OpenAI => {
                let api_key = api_key.ok_or_else(|| {
                    Error::Authentication("api_key is required for openai provider".to_string())
                })?;

                let client = openai::_OpenAI::_new(
                    api_key,
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Box::new(client)
            }
            _Providers::CustomSageMakerAI => {
                let client = sagemaker::_CustomSageMakerAI::_build_sagemaker_client(model).await;
                Box::new(client)
            }
            _Providers::LocalInference => {
                let client = local_inference::_LocalInferenceAI::_setup(
                    script_name.unwrap_or("inference".to_owned()),
                    function_handler.unwrap_or("predict".to_owned()), // Function name
                )
                .await;

                Box::new(client)
            }
        };

        Ok(client)
    }

    pub(crate) fn _init_with_schema<J: JsonSchema, T: Tool + 'static>(
        provider: _Providers,
        model: &str,
        api_key: Option<&str>,
        system_message: String,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<_ToolWrapper<T>>,
    ) -> Result<Box<dyn _CompletionProvider>, Error> {
        match provider {
            _Providers::Ollama => {
                let client = ollama::_OllamaAI::_new_with_schema::<J, T>(
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Ok(Box::new(client))
            }
            _Providers::OpenAI => {
                let api_key = api_key.ok_or_else(|| {
                    Error::Authentication("api_key is required for openai provider".to_string())
                })?;

                let client = openai::_OpenAI::_new_with_schema::<J, T>(
                    api_key,
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Ok(Box::new(client))
            }
            _Providers::CustomSageMakerAI => {
                unimplemented!("Does not support Schema responses")
            }
            _Providers::LocalInference => {
                unimplemented!("Does not support Schema responses")
            }
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct EmailRequest {
    pub from: String,
    pub subject: String,
    pub body: String,
}

impl Display for EmailRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "From: {}\nSubject: {}\nBody: {}",
            self.from, self.subject, self.body
        )
    }
}

impl MessageType for EmailRequest {
    fn msg_type(&self) -> Result<rpc_agent::Message, Error> {
        let message = serde_json::to_value(self).map_err(|e| Error::Other(e.to_string()))?;

        Ok(rpc_agent::Message::Struct(message))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpamRating {
    /// The label for the spam rating.
    pub label: Label,
    /// The confidence score, between 0 and 1.
    pub score: f32,
}

impl TryFrom<String> for SpamRating {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let res = if let Ok(response) = serde_json::from_str::<Vec<Self>>(&value) {
            response[0].clone()
        } else {
            serde_json::from_str::<Self>(&value)?
        };

        Ok(res)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum Label {
    #[serde(rename = "LABEL_0")]
    Ham,
    #[serde(rename = "LABEL_1")]
    Spam,
    #[serde(rename = "LABEL_2")]
    Phishing,
}

impl Serialize for Label {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = match self {
            Label::Ham => "Ham",
            Label::Spam => "Spam",
            Label::Phishing => "Phishing",
        };
        serializer.serialize_str(s)
    }
}
