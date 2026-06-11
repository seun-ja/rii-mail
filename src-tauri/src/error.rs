use std::{error::Error as StdError, io::ErrorKind};

use aws_sdk_sagemakerruntime::{error::SdkError, operation::invoke_endpoint::InvokeEndpointError};
use lettre::address::AddressError;
use pyo3::PyErr;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc::error::SendError, oneshot::error::RecvError};
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
    StateChannelSend(#[from] SendError<InitializedState>),
    #[error("Imap Config Channel Send Error: {0}")]
    ImapConfigChannelSend(#[from] SendError<ImapClientConfig>),
    #[error("Returning User Imap Config Channel Send Error: {0}")]
    ReturningUserImapConfigChannelSend(String),
    #[error("Imap Command Channel Send Error: {0}")]
    ImapCommandChannelSend(#[from] SendError<ImapCommand>),
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
    /// Keychain error: an error occurred while accessing the Apple keychain.
    #[error("keychain error: {0}")]
    #[cfg(target_os = "macos")]
    Keychain(#[from] security_framework::base::Error),
    /// Keychain error: Apple Keychain is only supported on macOS.
    #[error("keychain error: {0}")]
    #[cfg(not(target_os = "macos"))]
    Keychain(String),
    /// Byte conversion error: an error occurred while converting bytes to a string.
    #[error("byte conversion error: {0}")]
    ByteConversion(#[from] std::string::FromUtf8Error),
    #[error("Oneshot channel receive error: {0}")]
    OneShotRecv(#[from] RecvError),
    #[error("Thread cancellation fails")]
    ThreadCancel,
    #[error("SMTP Transport Relay Error: {0}")]
    SmtpTransportRelay(#[from] lettre::transport::smtp::Error),
    #[error("Error sending email: {0}")]
    SendEmail(#[from] lettre::error::Error),
    #[error("Error parsing Email Address")]
    AddressParse(#[from] AddressError),
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
            Error::ReturningUserImapConfigChannelSend(e) => {
                ("ReturningUserImapConfigChannelSend", e.to_string())
            }
            Error::ImapCommandChannelSend(e) => ("ImapCommandChannelSend", e.to_string()),
            Error::Authentication(e) => ("Authentication", e.to_string()),
            Error::Http(e) => ("Http", e.to_string()),
            Error::Prompt(e) => ("Prompt", e.to_string()),
            Error::LocalInference(e) => ("LocalInference", e.to_string()),
            Error::Invoke(e) => ("Invoke", e.to_string()),
            Error::Keychain(e) => ("Keychain", e.to_string()),
            Error::ByteConversion(e) => ("ByteConversion", e.to_string()),
            Error::OneShotRecv(e) => ("OneShotRecv", e.to_string()),
            Error::ThreadCancel => ("ThreadCancel", self.to_string()),
            Error::SmtpTransportRelay(e) => ("SmtpTransportRelay", e.to_string()),
            Error::SendEmail(e) => ("SendEmail", e.to_string()),
            Error::AddressParse(e) => ("AddressParse", e.to_string()),
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
            | Error::ReturningUserImapConfigChannelSend(_)
            | Error::ImapCommandChannelSend(_)
            | Error::Sqlx(_)
            | Error::Tls(_)
            | Error::Http(_)
            | Error::Prompt(_)
            | Error::LocalInference(_)
            | Error::Invoke(_)
            | Error::Keychain(_)
            | Error::OneShotRecv(_)
            | Error::ByteConversion(_)
            | Error::SmtpTransportRelay(_)
            | Error::SendEmail(_)
            | Error::AddressParse(_)
            | Error::ThreadCancel => {
                error!(error = ?self, "Error occurred: {}", self);
            }
            Error::NotFound | Error::Other(_) | Error::Authentication(_) => {
                warn!(error = ?self, "Error occurred: {}", self);
            }
        }
    }
}

pub type ErrorMessage = String;

pub(crate) fn friendly_login_error_message(stage: &str, raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();

    if lower.contains("authentication")
        || lower.contains("invalid credentials")
        || lower.contains("login failed")
        || lower.contains("auth")
    {
        return "Invalid email or password. Please verify your credentials and try again."
            .to_string();
    }

    if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("network")
        || lower.contains("refused")
    {
        return "Unable to reach the mail server. Check your internet connection and IMAP server settings."
            .to_string();
    }

    if lower.contains("tls") || lower.contains("certificate") || lower.contains("ssl") {
        return "Secure connection to the mail server failed. Please verify TLS/SSL settings."
            .to_string();
    }

    match stage {
        "init" => {
            "Could not connect to the IMAP server. Please check server host and port.".to_string()
        }
        "login" => "Login failed due to a server error. Please try again shortly.".to_string(),
        _ => "Login failed. Please try again.".to_string(),
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
