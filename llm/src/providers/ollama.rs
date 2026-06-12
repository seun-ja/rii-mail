use rig::{
    agent::{Agent, AgentBuilder, WithBuilderTools},
    client::{CompletionClient as _, Nothing},
    completion::Prompt as _,
    providers::ollama::{Client, CompletionModel},
    tool::Tool,
};
use schemars::JsonSchema;

use crate::{error::Error, providers::CompletionProvider, tools::ToolWrapper};

struct OllamaProvider {
    model: String,
}

impl OllamaProvider {
    fn new(model: String) -> Self {
        Self { model }
    }

    fn build<T: Tool + 'static>(
        &self,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<OllamaAI, Error> {
        let builder = builder(&self.model, system_message, temperature, max_tokens)?;

        let agent = if !tools.is_empty() {
            builder_with_tools(builder, tools)?.build()
        } else {
            builder.build()
        };

        Ok(OllamaAI { agent })
    }

    fn build_with_schema<J: JsonSchema, T: Tool + 'static>(
        &self,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<OllamaAI, Error> {
        let builder =
            builder(&self.model, system_message, temperature, max_tokens)?.output_schema::<J>();

        let agent = if !tools.is_empty() {
            builder_with_tools(builder, tools)?.build()
        } else {
            builder.build()
        };

        Ok(OllamaAI { agent })
    }
}

#[derive(Clone)]
pub struct OllamaAI {
    agent: Agent<CompletionModel>,
}

impl OllamaAI {
    pub fn new<T: Tool + 'static>(
        model: &str,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        let provider = OllamaProvider::new(model.to_string());

        provider.build(system_message, temperature, max_tokens, tools)
    }

    pub fn new_with_schema<J: JsonSchema, T: Tool + 'static>(
        model: &str,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        let provider = OllamaProvider::new(model.to_string());

        provider.build_with_schema::<J, T>(system_message, temperature, max_tokens, tools)
    }
}

#[async_trait::async_trait]
impl<J: JsonSchema + serde::de::DeserializeOwned> CompletionProvider<J> for OllamaAI {
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "ollama.chat", skip(self, prompt))
    )]
    async fn chat(&self, prompt: &str) -> Result<String, Error> {
        let response = self.agent.prompt(prompt).await?;
        Ok(response)
    }

    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "ollama.schema_chat", skip(self, prompt))
    )]
    async fn schema_chat(&self, prompt: &str) -> Result<J, Error> {
        let response = self.agent.prompt(prompt).await?;
        let res = serde_json::from_str(&response)?;
        Ok(res)
    }
}

fn builder(
    model: &str,
    system_message: Option<&str>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
) -> Result<AgentBuilder<CompletionModel>, Error> {
    let base_url = std::env::var("OLLAMA_API_BASE_URL").unwrap_or_else(|_| {
        #[cfg(feature = "tracing")]
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

fn builder_with_tools<T: Tool + 'static>(
    builder: AgentBuilder<CompletionModel>,
    tools: Vec<ToolWrapper<T>>,
) -> Result<AgentBuilder<CompletionModel, (), WithBuilderTools>, Error> {
    let mut tools = tools.into_iter();

    let Some(first) = tools.next() else {
        return Err(Error::Other("Empty list".to_string()));
    };

    let builder = tools.fold(builder.tool(*first.tool()), |builder, tool| {
        builder.tool(*tool.tool())
    });

    Ok(builder)
}
