use rig::{
    agent::{Agent, AgentBuilder, WithBuilderTools},
    client::{CompletionClient as _, Nothing},
    completion::Prompt as _,
    providers::ollama::{Client, CompletionModel},
    tool::Tool,
};
use schemars::JsonSchema;

use crate::{
    error::Error,
    llm::{_CompletionProvider, tools::_ToolWrapper},
};

struct _OllamaProvider {
    model: String,
}

impl _OllamaProvider {
    fn _new(model: String) -> Self {
        Self { model }
    }

    fn _build<T: Tool + 'static>(
        &self,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<_ToolWrapper<T>>,
    ) -> Result<_OllamaAI, Error> {
        let builder = _builder(&self.model, system_message, temperature, max_tokens)?;

        let agent = if let Some(tool) = tool {
            _builder_with_tools(builder, tool)?.build()
        } else {
            builder.build()
        };

        Ok(_OllamaAI { agent })
    }

    fn _build_with_schema<J: JsonSchema, T: Tool + 'static>(
        &self,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<_ToolWrapper<T>>,
    ) -> Result<_OllamaAI, Error> {
        let builder =
            _builder(&self.model, system_message, temperature, max_tokens)?.output_schema::<J>();

        let agent = if let Some(tool) = tool {
            _builder_with_tools(builder, tool)?.build()
        } else {
            builder.build()
        };

        Ok(_OllamaAI { agent })
    }
}

#[derive(Clone)]
pub struct _OllamaAI {
    agent: Agent<CompletionModel>,
}

impl _OllamaAI {
    pub fn _new<T: Tool + 'static>(
        model: &str,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<_ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        let provider = _OllamaProvider::_new(model.to_string());

        provider._build(system_message, temperature, max_tokens, tool)
    }

    pub fn _new_with_schema<J: JsonSchema, T: Tool + 'static>(
        model: &str,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tool: Option<_ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        let provider = _OllamaProvider::_new(model.to_string());

        provider._build_with_schema::<J, T>(system_message, temperature, max_tokens, tool)
    }
}

#[async_trait::async_trait]
impl _CompletionProvider for _OllamaAI {
    #[tracing::instrument(name = "ollama.chat", skip(self, prompt))]
    async fn chat(&self, prompt: &str) -> Result<String, Error> {
        let response = self.agent.prompt(prompt).await?;
        Ok(response)
    }
}

fn _builder(
    model: &str,
    system_message: Option<&str>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
) -> Result<AgentBuilder<CompletionModel>, Error> {
    let base_url = std::env::var("OLLAMA_API_BASE_URL").unwrap_or_else(|_| {
        tracing::warn!("OLLAMA_API_BASE_URL not set, using default: http://localhost:11434");
        "http://localhost:11434".to_string()
    });

    let ollama_client = Client::builder()
        .api_key(Nothing)
        .base_url(&base_url)
        .build()?;

    let builder = ollama_client
        .agent(model)
        .preamble(system_message.unwrap_or_default())
        .temperature(temperature.unwrap_or_default())
        .max_tokens(max_tokens.unwrap_or_default());

    Ok(builder)
}

fn _builder_with_tools<T: Tool + 'static>(
    builder: AgentBuilder<CompletionModel>,
    tool: _ToolWrapper<T>,
) -> Result<AgentBuilder<CompletionModel, (), WithBuilderTools>, Error> {
    let builder = builder.tool(*tool._tool());

    Ok(builder)
}
