use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use llm::{
    builder::AgentBuilder,
    providers::Providers,
    tools::{NoTool, ToolWrapper},
    Agent,
};
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

use crate::{
    error::Error,
    llm::system_messages::{EMAIL_LLM_SYSTEM_MESSAGE, SPAM_RATER_SYSTEM_MESSAGE},
};

mod system_messages;

#[derive(Deserialize, Debug, Serialize, JsonSchema)]
pub struct Email {
    pub from: String,
    pub subject: String,
    pub body: String,
}

impl Display for Email {
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
            response
                .into_iter()
                .next()
                .ok_or_else(|| Error::Other("spam rating array cannot be empty".to_string()))?
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

pub(crate) fn resolve_local_inference_paths(
    resource_root: &Path,
) -> Result<(PathBuf, PathBuf), Error> {
    let model_resource = resource_root.join("local-model");
    let model_path = model_resource.canonicalize().map_err(|error| {
        Error::Other(format!(
            "bundled local model is unavailable at {}: {error}",
            model_resource.display()
        ))
    })?;
    let script_resource = resource_root.join("llm/local_inference.py");
    let script_path = script_resource.canonicalize().map_err(|error| {
        Error::Other(format!(
            "bundled Python inference script is unavailable at {}: {error}",
            script_resource.display()
        ))
    })?;

    Ok((model_path, script_path))
}

#[tracing::instrument(name = "llm.local.initialize", skip_all, err)]
pub async fn initialize_local_inference(resource_root: &Path) -> Result<Agent<Email>, Error> {
    let (model_path, script_path) = resolve_local_inference_paths(resource_root)?;
    let model_path = model_path
        .to_str()
        .ok_or_else(|| Error::Other("model path is not valid UTF-8".to_string()))?;
    let script_path = script_path
        .to_str()
        .ok_or_else(|| Error::Other("Python script path is not valid UTF-8".to_string()))?;

    let agent: Agent<Email> = AgentBuilder::<NoTool>::new(
        Providers::LocalInference,
        SPAM_RATER_SYSTEM_MESSAGE,
        model_path,
    )
    .script_name(script_path.to_string())
    .build()
    .await?;

    Ok(agent)
}

#[tracing::instrument(name = "llm.agent.initialize", skip_all, err)]
pub fn init_llm_agent<T: Tool>(
    provider: Option<Providers>,
    model: &str,
    api_key: &str,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    tools: Vec<ToolWrapper<T>>,
) -> Result<Option<Agent<Email>>, Error> {
    match provider {
        Some(p) => {
            AgentBuilder::new(p, EMAIL_LLM_SYSTEM_MESSAGE, model) // TODO: work on the system message
                .api_key(api_key)
                .temperature(temperature.unwrap_or(0.0))
                .max_tokens(max_tokens.unwrap_or(3000))
                .tools(tools)
                .build_with_schema::<Email>()
                .map(|agent| Ok(Some(agent)))?
        }
        None => Ok(None),
    }
}
