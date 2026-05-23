pub mod local_inference;
pub mod ollama;
pub mod openai;
pub mod sagemaker;

use rig::tool::Tool;
use schemars::JsonSchema;

use crate::{
    error::Error,
    llm::{
        _LlmProvider, _Providers,
        tools::{_NoTool, _ToolWrapper},
    },
};

/// Builder for creating an [`AgentServer`].
pub struct _ProviderBuilder<'a> {
    provider: _Providers,
    system_message: &'a str,
    model: &'a str,
    api_key: Option<&'a str>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    script_name: Option<String>,
    function_handler: Option<String>,
}

impl<'a> _ProviderBuilder<'a> {
    /// Creates a new [`AgentServerBuilder`] with the given provider, system message, and model.
    pub fn _new(provider: _Providers, system_message: &'a str, model: &'a str) -> Self {
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
    pub fn _api_key(mut self, api_key: &'a str) -> Self {
        self.api_key = Some(api_key);
        self
    }

    /// Sets the temperature for the provider.
    #[inline]
    pub fn _temperature(mut self, temperature: Option<f64>) -> Self {
        self.temperature = temperature;
        self
    }

    /// Sets the maximum number of tokens for the provider.
    #[inline]
    pub fn _max_tokens(mut self, max_tokens: Option<u64>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Sets the Python path for the provider.
    #[inline]
    pub fn _script_name(mut self, python_path: String) -> Self {
        self.script_name = Some(python_path);
        self
    }

    /// Sets the Python path for the provider.
    #[inline]
    pub fn _function_handler(mut self, function_handler: String) -> Self {
        self.function_handler = Some(function_handler);
        self
    }

    /// Builds the [`LlmProvider`] with the given configuration.
    pub async fn _build(self) -> Result<_LlmProvider, Error> {
        let providers = _Providers::_init::<_NoTool>(
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

        Ok(_LlmProvider { providers })
    }

    /// Builds the [`LlmProvider`] with the given configuration and schema.
    pub fn _build_with_schema<J: JsonSchema>(self) -> Result<_LlmProvider, Error> {
        let providers = _Providers::_init_with_schema::<J, _NoTool>(
            self.provider,
            self.model,
            self.api_key,
            self.system_message.to_string(),
            self.temperature,
            self.max_tokens,
            None,
        )?;

        Ok(_LlmProvider { providers })
    }

    /// Builds the [`LlmProvider`] with the given configuration and tool.
    pub async fn _build_with_tool<T: Tool + 'static>(
        self,
        tool: _ToolWrapper<T>,
    ) -> Result<_LlmProvider, Error> {
        let providers = _Providers::_init::<T>(
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

        Ok(_LlmProvider { providers })
    }
}
