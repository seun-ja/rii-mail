use rig::{
    agent::{Agent, AgentBuilder, WithBuilderTools},
    client::CompletionClient as _,
    completion::Prompt,
    providers::openai::{self, responses_api::ResponsesCompletionModel},
    tool::Tool,
};
use schemars::JsonSchema;

use crate::{error::Error, providers::CompletionProvider, tools::ToolWrapper};

struct OpenAIProvider {
    api_key: String,
    model: String,
}

impl OpenAIProvider {
    fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }

    fn build<T: Tool + 'static>(
        &self,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<OpenAI, Error> {
        let builder = builder(
            &self.api_key,
            &self.model,
            system_message,
            temperature,
            max_tokens,
        )?;

        let agent = if !tools.is_empty() {
            builder_with_tools(builder, tools)?.build()
        } else {
            builder.build()
        };

        Ok(OpenAI { agent })
    }

    fn build_with_schema<J: JsonSchema, T: Tool + 'static>(
        &self,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<OpenAI, Error> {
        let builder = builder(
            &self.api_key,
            &self.model,
            system_message,
            temperature,
            max_tokens,
        )?
        .output_schema::<J>();

        let agent = if !tools.is_empty() {
            builder_with_tools(builder, tools)?.build()
        } else {
            builder.build()
        };

        Ok(OpenAI { agent })
    }
}

#[derive(Clone)]
pub struct OpenAI {
    agent: Agent<ResponsesCompletionModel>,
}

impl OpenAI {
    pub fn new<T: Tool + 'static>(
        api_key: &str,
        model: &str,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        let provider = OpenAIProvider::new(api_key.to_string(), model.to_string());

        provider.build(system_message, temperature, max_tokens, tools)
    }

    pub fn new_with_schema<J: JsonSchema, T: Tool + 'static>(
        api_key: &str,
        model: &str,
        system_message: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
        tools: Vec<ToolWrapper<T>>,
    ) -> Result<Self, Error> {
        let provider = OpenAIProvider::new(api_key.to_string(), model.to_string());

        provider.build_with_schema::<J, T>(system_message, temperature, max_tokens, tools)
    }
}

#[async_trait::async_trait]
impl<J: JsonSchema + serde::de::DeserializeOwned> CompletionProvider<J> for OpenAI {
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "openai.chat", skip(self, prompt))
    )]
    async fn chat(&self, prompt: &str) -> Result<String, Error> {
        let response = self.agent.prompt(prompt).await?;
        Ok(response)
    }

    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "openai.schema_chat", skip(self, prompt))
    )]
    async fn schema_chat(&self, prompt: &str) -> Result<J, Error> {
        let response = self.agent.prompt(prompt).await?;
        let res = serde_json::from_str(&response)?;
        Ok(res)
    }
}

fn builder(
    api_key: &str,
    model: &str,
    system_message: Option<&str>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
) -> Result<AgentBuilder<ResponsesCompletionModel>, Error> {
    let openai_client = openai::Client::new(api_key)?;

    let builder = openai_client
        .agent(model)
        .preamble(system_message.unwrap_or_default())
        .temperature(temperature.unwrap_or_default())
        .max_tokens(max_tokens.unwrap_or_default());

    Ok(builder)
}

fn builder_with_tools<T: Tool + 'static>(
    builder: AgentBuilder<ResponsesCompletionModel>,
    tools: Vec<ToolWrapper<T>>,
) -> Result<AgentBuilder<ResponsesCompletionModel, (), WithBuilderTools>, Error> {
    let mut tools = tools.into_iter();

    let Some(first) = tools.next() else {
        return Err(Error::Other("Empty list".to_string()));
    };

    let builder = tools.fold(builder.tool(*first.tool()), |builder, tool| {
        builder.tool(*tool.tool())
    });

    Ok(builder)
}
