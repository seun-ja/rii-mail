use std::{path::PathBuf, sync::Arc};

use crate::{
    auth::AppleKeychainManager,
    db::Provider,
    error::{Error, ErrorMessage},
    rpc_llm::{AgentWorkerClient, Email},
};
use lettre::{transport::smtp::authentication::Credentials, SmtpTransport};
use llm::{builder::AgentBuilder, providers::Providers, tools::ToolWrapper, Agent};
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tarpc::{client, serde_transport::tcp, tokio_serde::formats::Json};
use tokio::fs;

const SYSTEM_MESSAGE: &str = r#" You are an expert executive assistant and professional email writer.

Your job is to transform the user's thoughts, notes, instructions, and context into a complete, polished email.

Rules:

- Generate professional, natural, human-sounding emails.
- Adapt tone based on the user's intent:
  - Formal business communication
  - Casual communication
  - Customer support
  - Follow-up emails
  - Sales outreach
  - Recruiting communication
  - Internal team communication
  - Apologies
  - Thank-you messages
  - Status updates
  - Meeting requests

- Never mention that you are an AI.
- Never explain your reasoning.
- Never return commentary about the email.
- Never include placeholders such as "[Name]" unless the user explicitly asks for placeholders.
- Never invent facts, dates, numbers, commitments, pricing, or names that were not provided.
- When information is missing, write naturally and avoid making assumptions.

Subject Guidelines:
- Create a concise, specific subject line.
- Avoid generic subjects like "Hello" or "Quick Question" unless appropriate.
- Keep subjects under 12 words whenever possible.

Body Guidelines:
- Start with an appropriate greeting.
- Write clear and concise paragraphs.
- Use professional business writing standards.
- End with an appropriate closing.
- Keep the email focused on the user's objective.
- Eliminate unnecessary fluff.
- Ensure grammar, spelling, and punctuation are correct.

Recipient Extraction:
- If the user explicitly provides recipient email addresses, place them in the appropriate fields.
- If no recipient is provided, leave recipient fields empty.
- Never guess email addresses.

CC/BCC:
- Only populate CC or BCC when explicitly requested.
- Otherwise leave them empty.

Output Requirements:
- Generate the best possible version of the email.
- Ensure the email is ready to send without further editing.
- Prefer clarity over complexity.
- Prefer professionalism over cleverness.

The generated email should feel like it was written by an experienced professional assistant who understands modern business communication.
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InitStatus {
    /// Configuration file doesn't exist - user needs to complete full setup
    Setup,
    /// Configuration exists but user is not authenticated - show login page
    ///
    /// In case where there's a redirect because of error, there would be `Some`
    Login(Option<ErrorMessage>),
    /// User is fully authenticated and app is ready
    SignedIn,
    /// User is returning and already authenticated
    ReturningSigned,
}

#[derive(Clone)]
pub struct Account {
    pub name: Option<String>,
    pub email: String,
    pub provider: Provider,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub rpc_server_url: String,
    pub imap_server_url: String,
    pub smtp_relay_url: String,
    pub imap_port: u16,
    pub sqlite_db: String,
    pub accounts: Vec<String>,
    pub provider: String,
    pub default_model: String,
    pub api_key: String,
}

impl Config {
    pub async fn init(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_json = fs::read_to_string(path).await?;
        let config: Self = serde_json::from_str(&config_json)?;

        if config.imap_port == 0 || config.imap_server_url.is_empty() {
            return Err("IMAP Port and Server not set".into());
        }

        Ok(config)
    }
}

pub enum AppState {
    Initialized(Arc<InitializedState>),
    Fresh,
}

/// Coordinates startup status requests so the Keychain is read at most once
/// while the app is determining whether a returning user is signed in.
pub struct StartupStatusCache(pub tokio::sync::Mutex<Option<InitStatus>>);

impl AppState {
    pub fn state(&self) -> &InitializedState {
        match self {
            AppState::Initialized(state) => state,
            AppState::Fresh => panic!("Application not initialized"),
        }
    }
}

pub struct InitializedState {
    // pub _llm_client: LlmProvider,
    pub rpc_llm_client: AgentWorkerClient,
    pub sqlite_pool: SqlitePool,
    pub apple_keychain_manager: AppleKeychainManager,
    pub smtp_transport_client: SmtpTransport,
    pub llm_agent: Agent<Email>,
    pub accounts: Vec<Account>,
}

pub fn init_llm_agent<T: Tool>(
    provider: Providers,
    model: &str,
    api_key: &str,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    tools: Vec<ToolWrapper<T>>,
) -> Result<Agent<Email>, Error> {
    let agent = AgentBuilder::new(provider, SYSTEM_MESSAGE, model) // TODO: work on the system message
        .api_key(api_key)
        .temperature(temperature.unwrap_or(0.0))
        .max_tokens(max_tokens.unwrap_or(3000))
        .tools(tools)
        .build_with_schema::<Email>()?;

    Ok(agent)
}

pub async fn init_rpc(rpc_server: &str) -> Result<AgentWorkerClient, Error> {
    let transport = tcp::connect(rpc_server, Json::default).await?;

    let client = AgentWorkerClient::new(client::Config::default(), transport).spawn();

    Ok(client)
}

pub fn smtp_transport_client(
    smtp_server_url: &str,
    creds: Credentials,
) -> Result<SmtpTransport, Error> {
    let transport = SmtpTransport::relay(smtp_server_url)?
        .credentials(creds)
        .build();

    Ok(transport)
}

pub struct ImapClientConfig {
    pub username: String,
    pub password: String,
    pub imap_server_url: String,
    pub imap_port: u16,
    pub sqlite_pool: SqlitePool,
    pub login_result_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
}
