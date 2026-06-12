use aws_sdk_sagemakerruntime::{error::SdkError, operation::invoke_endpoint::InvokeEndpointError};
use pyo3::PyErr;
use thiserror::Error;

/// Error type used throughout the crate.
#[derive(Error, Debug)]
pub enum Error {
    /// Provider error: the provider returned an error response.
    #[error("provider error: {0}")]
    ProviderError(String),
    /// Authentication error: the provider returned an authentication error.
    #[error("authentication error: {0}")]
    AuthenticationError(String),
    /// Client error: the HTTP client returned an error.
    #[error("client error: {0}")]
    HttpError(#[from] rig::http_client::Error),
    /// Prompt error: the prompt returned an error.
    #[error("prompt error: {0}")]
    PromptError(#[from] rig::completion::PromptError),
    /// IO error: an I/O error occurred.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
    /// Invoke error: an AWS error occurred during the invoke endpoint operation.
    #[error("invoke error: an AWS error occurred during the invoke endpoint operation: {0:?}")]
    InvokeError(#[from] Box<SdkError<InvokeEndpointError>>),
    /// Serialization error: failed to serialize the request payload.
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("local inference error: {0}")]
    LocalInferenceError(#[from] PyErr),
}
