use std::sync::Arc;

use rig::tool::Tool;
use schemars::JsonSchema;

use crate::{error::Error, providers::Providers, tools::ToolWrapper, Agent};

/// Builder for creating an [`Agent`].
pub struct AgentBuilder<'a, T: Tool + 'static> {
    provider: Providers,
    system_message: &'a str,
    model: &'a str,
    api_key: Option<&'a str>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    script_name: Option<String>,
    function_handler: Option<String>,
    tools: Vec<ToolWrapper<T>>,
}

impl<'a, T: Tool + 'static> AgentBuilder<'a, T> {
    /// Creates a new [`AgentBuilder`] with the given port, provider, system message, and model.
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
            tools: vec![],
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
    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Sets the maximum number of tokens for the provider.
    #[inline]
    pub fn max_tokens(mut self, max_tokens: u64) -> Self {
        self.max_tokens = Some(max_tokens);
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

    #[inline]
    pub fn tool(mut self, tool: ToolWrapper<T>) -> Self {
        self.tools.push(tool);
        self
    }

    #[inline]
    pub fn tools(mut self, tools: Vec<ToolWrapper<T>>) -> Self {
        self.tools.extend(tools);
        self
    }

    /// Builds the [`Agent`] with the given configuration.
    pub async fn build<J: serde::de::DeserializeOwned + JsonSchema>(
        self,
    ) -> Result<Agent<J>, Error> {
        let providers = Providers::init::<T, J>(
            self.provider,
            self.model,
            self.api_key,
            self.system_message.to_string(),
            self.temperature,
            self.max_tokens,
            self.tools,
            self.script_name,
            self.function_handler,
        )
        .await?;

        Ok(Agent(Arc::new(providers)))
    }

    /// Builds the [`Agent`] with the given configuration and schema.
    pub fn build_with_schema<J: JsonSchema + serde::de::DeserializeOwned>(
        self,
    ) -> Result<Agent<J>, Error> {
        let providers = Providers::init_with_schema::<T, J>(
            self.provider,
            self.model,
            self.api_key,
            self.system_message.to_string(),
            self.temperature,
            self.max_tokens,
            self.tools,
        )?;

        Ok(Agent(Arc::new(providers)))
    }
}
