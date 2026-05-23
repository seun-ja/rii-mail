use std::{error::Error as StdError, io::ErrorKind};

use aws_sdk_sagemakerruntime::{error::SdkError, operation::invoke_endpoint::InvokeEndpointError};
use pyo3::PyErr;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::{
    config::{ImapClientConfig, InitializedState},
    imap::ImapCommand,
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Serialization Error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Error: {0}")]
    Other(String),
    #[error("Not Found")]
    NotFound,
    #[error("RPC Server Error: {0}")]
    RPCServer(#[from] rpc_agent::error::ApiError),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS Error: {0}")]
    Tls(#[from] async_native_tls::Error),
    #[error("IMAP Error: {0}")]
    Imap(#[from] async_imap::error::Error),
    #[error("Tauri Error: {0}")]
    Tauri(#[from] tauri::Error),
    #[error("SQLx Error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("State Channel Send Error: {0}")]
    StateChannelSend(#[from] mpsc::error::SendError<InitializedState>),
    #[error("Imap Config Channel Send Error: {0}")]
    ImapConfigChannelSend(#[from] mpsc::error::SendError<ImapClientConfig>),
    #[error("Imap Command Channel Send Error: {0}")]
    ImapCommandChannelSend(#[from] mpsc::error::SendError<ImapCommand>),
    /// Authentication error: the provider returned an authentication error.
    #[error("authentication error: {0}")]
    Authentication(String),
    /// Client error: the HTTP client returned an error.
    #[error("client error: {0}")]
    Http(#[from] rig::http_client::Error),
    /// Prompt error: the prompt returned an error.
    #[error("prompt error: {0}")]
    Prompt(#[from] rig::completion::PromptError),
    #[error("local inference error: {0}")]
    LocalInference(#[from] PyErr),
    /// Invoke error: an AWS error occurred during the invoke endpoint operation.
    #[error("invoke error: an AWS error occurred during the invoke endpoint operation: {0:?}")]
    Invoke(#[from] Box<SdkError<InvokeEndpointError>>),
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (kind, message) = match self {
            Error::Serialization(e) => ("Serialization", e.to_string()),
            Error::Other(msg) => ("Other", msg.clone()),
            Error::NotFound => ("NotFound", self.to_string()),
            Error::RPCServer(e) => ("RPCServer", e.to_string()),
            Error::Io(e) => ("Io", e.to_string()),
            Error::Tls(e) => ("Tls", e.to_string()),
            Error::Imap(e) => ("Imap", e.to_string()),
            Error::Tauri(e) => ("Tauri", e.to_string()),
            Error::Sqlx(e) => ("SQLx", e.to_string()),
            Error::StateChannelSend(e) => ("StateChannelSend", e.to_string()),
            Error::ImapConfigChannelSend(e) => ("ImapConfigChannelSend", e.to_string()),
            Error::ImapCommandChannelSend(e) => ("ImapCommandChannelSend", e.to_string()),
            Error::Authentication(e) => ("AuthenticationError", e.to_string()),
            Error::Http(e) => ("HttpError", e.to_string()),
            Error::Prompt(e) => ("PromptError", e.to_string()),
            Error::LocalInference(e) => ("LocalInferenceError", e.to_string()),
            Error::Invoke(e) => ("InvokeError", e.to_string()),
        };

        use serde::ser::SerializeStruct;

        self.log_error();

        let mut state = serializer.serialize_struct("Error", 2)?;
        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &message)?;
        state.end()
    }
}

impl Error {
    /// Log the error using tracing at error level, including its source chain.
    pub fn log_error(&self) {
        match self {
            Error::Serialization(_)
            | Error::RPCServer(_)
            | Error::Io(_)
            | Error::Imap(_)
            | Error::Tauri(_)
            | Error::StateChannelSend(_)
            | Error::ImapConfigChannelSend(_)
            | Error::ImapCommandChannelSend(_)
            | Error::Sqlx(_)
            | Error::Tls(_)
            | Error::Http(_)
            | Error::Prompt(_)
            | Error::LocalInference(_)
            | Error::Invoke(_) => {
                error!(error = ?self, "Error occurred: {}", self);
            }
            Error::NotFound | Error::Other(_) | Error::Authentication(_) => {
                warn!(error = ?self, "Error occurred: {}", self);
            }
        }
    }
}

pub enum CrashAction {
    Retry { backoff_ms: u64, reason: String },
    Fatal,
    Ignore,
}

#[derive(Debug, Deserialize)]
struct RetryContext<'a> {
    message: &'a str,
    reason: &'a str,
}

pub fn inference_crash_handler(err: &str, attempt: u32) -> CrashAction {
    if let Ok(err_json) = serde_json::from_str::<RetryContext>(err) {
        match err_json.message {
            "MODEL_OOM" => {
                let backoff = 100 * (attempt + 1) as u64;
                return CrashAction::Retry {
                    backoff_ms: backoff,
                    reason: err_json.reason.to_string(),
                };
            }
            "MODEL_FATAL" => {
                return CrashAction::Fatal;
            }
            _ => {}
        }
    }
    CrashAction::Ignore
}

/// Returns true if the error or any of its sources is a connection error (BrokenPipe, ConnectionReset, ConnectionAborted, NotConnected)
pub fn is_connection_error(e: &(dyn StdError + 'static)) -> bool {
    if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
        matches!(
            io_err.kind(),
            ErrorKind::BrokenPipe
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::NotConnected
        )
    } else if let Some(source) = e.source() {
        is_connection_error(source)
    } else {
        false
    }
}
