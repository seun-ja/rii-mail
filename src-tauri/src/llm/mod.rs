use std::fmt::Display;

use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

use crate::{
    error::Error,
    llm::{
        providers::{local_inference, ollama, openai, sagemaker},
        tools::ToolWrapper,
    },
};

mod providers;
mod tools;

pub use providers::ProviderBuilder;

// #[cfg(test)]
// pub use providers::local_inference::LocalInferenceAI;

pub struct LlmProvider {
    providers: Box<dyn CompletionProvider>,
}

impl LlmProvider {
    #[tracing::instrument(name = "agent.message", skip(self, email))]
    pub async fn chat(&self, email: EmailRequest) -> Result<String, Error> {
        self.providers.chat(&email.to_string()).await
    }
}

#[async_trait::async_trait]
pub trait CompletionProvider: Send + Sync {
    /// Returns a chat response for the given prompt.
    async fn chat(&self, prompt: &str) -> Result<String, Error>;
}

/// Supported AI providers for the agent server.
pub enum Providers {
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

impl Display for Providers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Providers::Ollama => write!(f, "ollama"),
            Providers::OpenAI => write!(f, "openai"),
            Providers::CustomSageMakerAI => write!(f, "sagemaker"),
            Providers::LocalInference => write!(f, "local"),
        }
    }
}

impl From<&str> for Providers {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "ollama" => Providers::Ollama,
            "openai" => Providers::OpenAI,
            "sagemaker" => Providers::CustomSageMakerAI,
            "local" => Providers::LocalInference,
            _ => panic!(
                "unknown provider: {}. Currently supported providers are: ollama, openai, sagemaker, local",
                value
            ),
        }
    }
}

impl Providers {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn init<T: Tool + 'static>(
        provider: Providers,
        model: &str,
        api_key: Option<&str>,
        system_message: String,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<ToolWrapper<T>>,
        script_name: Option<String>,
        function_handler: Option<String>,
    ) -> Result<Box<dyn CompletionProvider>, Error> {
        let client: Box<dyn CompletionProvider> = match provider {
            Providers::Ollama => {
                let client = ollama::OllamaAI::new(
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Box::new(client)
            }
            Providers::OpenAI => {
                let api_key = api_key.ok_or_else(|| {
                    Error::Authentication("api_key is required for openai provider".to_string())
                })?;

                let client = openai::OpenAI::new(
                    api_key,
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Box::new(client)
            }
            Providers::CustomSageMakerAI => {
                let client = sagemaker::CustomSageMakerAI::build_sagemaker_client(model).await;
                Box::new(client)
            }
            Providers::LocalInference => {
                let client = local_inference::LocalInferenceAI::setup(
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
        provider: Providers,
        model: &str,
        api_key: Option<&str>,
        system_message: String,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<ToolWrapper<T>>,
    ) -> Result<Box<dyn CompletionProvider>, Error> {
        match provider {
            Providers::Ollama => {
                let client = ollama::OllamaAI::_new_with_schema::<J, T>(
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Ok(Box::new(client))
            }
            Providers::OpenAI => {
                let api_key = api_key.ok_or_else(|| {
                    Error::Authentication("api_key is required for openai provider".to_string())
                })?;

                let client = openai::OpenAI::_new_with_schema::<J, T>(
                    api_key,
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tool,
                )?;
                Ok(Box::new(client))
            }
            Providers::CustomSageMakerAI => {
                unimplemented!("Does not support Schema responses")
            }
            Providers::LocalInference => {
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
