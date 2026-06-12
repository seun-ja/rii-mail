use std::fmt::Display;

use rig::tool::Tool;
use schemars::JsonSchema;

use crate::{error::Error, tools::ToolWrapper};

mod local_inference;
mod ollama;
mod openai;
mod sagemaker;

#[cfg(test)]
pub use local_inference::LocalInferenceAI;

#[async_trait::async_trait]
pub trait CompletionProvider<J: JsonSchema + serde::de::DeserializeOwned>: Send + Sync {
    /// Returns a chat response for the given prompt.
    async fn chat(&self, prompt: &str) -> Result<String, Error>;
    async fn schema_chat(&self, prompt: &str) -> Result<J, Error>;
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
    pub(crate) async fn init<T: Tool + 'static, J: JsonSchema + serde::de::DeserializeOwned>(
        provider: Providers,
        model: &str,
        api_key: Option<&str>,
        system_message: String,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
        script_name: Option<String>,
        function_handler: Option<String>,
    ) -> Result<Box<dyn CompletionProvider<J>>, Error> {
        let client: Box<dyn CompletionProvider<J>> = match provider {
            Providers::Ollama => {
                let client = ollama::OllamaAI::new(
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tools,
                )?;
                Box::new(client)
            }
            Providers::OpenAI => {
                let api_key = api_key.ok_or_else(|| {
                    Error::AuthenticationError(
                        "api_key is required for openai provider".to_string(),
                    )
                })?;

                let client = openai::OpenAI::new(
                    api_key,
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tools,
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
                );

                Box::new(client)
            }
        };

        Ok(client)
    }

    pub(crate) fn init_with_schema<
        T: Tool + 'static,
        J: JsonSchema + serde::de::DeserializeOwned,
    >(
        provider: Providers,
        model: &str,
        api_key: Option<&str>,
        system_message: String,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<Box<dyn CompletionProvider<J>>, Error> {
        match provider {
            Providers::Ollama => {
                let client = ollama::OllamaAI::new_with_schema::<J, T>(
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tools,
                )?;
                Ok(Box::new(client))
            }
            Providers::OpenAI => {
                let api_key = api_key.ok_or_else(|| {
                    Error::AuthenticationError(
                        "api_key is required for openai provider".to_string(),
                    )
                })?;

                let client = openai::OpenAI::new_with_schema::<J, T>(
                    api_key,
                    model,
                    Some(&system_message),
                    temperature,
                    max_tokens,
                    tools,
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
