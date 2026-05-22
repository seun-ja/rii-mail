pub mod local_inference;
pub mod ollama;
pub mod openai;
pub mod sagemaker;

use rig::tool::Tool;
use schemars::JsonSchema;

use crate::{
    error::Error,
    llm::{
        tools::{NoTool, ToolWrapper},
        LlmProvider, Providers,
    },
};

/// Builder for creating an [`AgentServer`].
pub struct ProviderBuilder<'a> {
    provider: Providers,
    system_message: &'a str,
    model: &'a str,
    api_key: Option<&'a str>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    script_name: Option<String>,
    function_handler: Option<String>,
}

impl<'a> ProviderBuilder<'a> {
    /// Creates a new [`AgentServerBuilder`] with the given provider, system message, and model.
    pub fn new(provider: Providers, system_message: &'a str, model: &'a str) -> Self {
        Self {
            provider,
            system_message,
            model,
            api_key: None,
            temperature: None,
            max_tokens: None,
            script_name: None,
            function_handler: None,
        }
    }

    /// Sets the API key for the provider.
    #[inline]
    pub fn api_key(mut self, api_key: &'a str) -> Self {
        self.api_key = Some(api_key);
        self
    }

    /// Sets the temperature for the provider.
    #[inline]
    pub fn temperature(mut self, temperature: Option<f64>) -> Self {
        self.temperature = temperature;
        self
    }

    /// Sets the maximum number of tokens for the provider.
    #[inline]
    pub fn max_tokens(mut self, max_tokens: Option<u64>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Sets the Python path for the provider.
    #[inline]
    pub fn script_name(mut self, python_path: String) -> Self {
        self.script_name = Some(python_path);
        self
    }

    /// Sets the Python path for the provider.
    #[inline]
    pub fn function_handler(mut self, function_handler: String) -> Self {
        self.function_handler = Some(function_handler);
        self
    }

    /// Builds the [`LlmProvider`] with the given configuration.
    pub async fn build(self) -> Result<LlmProvider, Error> {
        let providers = Providers::init::<NoTool>(
            self.provider,
            self.model,
            self.api_key,
            self.system_message.to_string(),
            self.temperature,
            self.max_tokens,
            None,
            self.script_name,
            self.function_handler,
        )
        .await?;

        Ok(LlmProvider { providers })
    }

    /// Builds the [`LlmProvider`] with the given configuration and schema.
    pub fn _build_with_schema<J: JsonSchema>(self) -> Result<LlmProvider, Error> {
        let providers = Providers::_init_with_schema::<J, NoTool>(
            self.provider,
            self.model,
            self.api_key,
            self.system_message.to_string(),
            self.temperature,
            self.max_tokens,
            None,
        )?;

        Ok(LlmProvider { providers })
    }

    /// Builds the [`LlmProvider`] with the given configuration and tool.
    pub async fn _build_with_tool<T: Tool + 'static>(
        self,
        tool: ToolWrapper<T>,
    ) -> Result<LlmProvider, Error> {
        let providers = Providers::init::<T>(
            self.provider,
            self.model,
            self.api_key,
            self.system_message.to_string(),
            self.temperature,
            self.max_tokens,
            Some(tool),
            None,
            None,
        )
        .await?;

        Ok(LlmProvider { providers })
    }
}
