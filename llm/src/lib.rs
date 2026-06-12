use std::sync::Arc;

use crate::providers::CompletionProvider;

pub mod builder;
pub mod error;
pub mod providers;
pub mod tools;

use std::fmt::Display;

use serde_json::Value;

/// Represents a message to be sent to the AI provider.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Message {
    /// A plain text message.
    Text(String),
    /// A structured message.
    Struct(Value),
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Text(text) => write!(f, "{text}"),
            Message::Struct(value) => {
                if let Ok(json) = serde_json::to_string_pretty(&value) {
                    write!(f, "{json}")
                } else {
                    write!(f, "{}", value.to_string())
                }
            }
        }
    }
}

///
#[derive(Clone)]
pub struct Agent<D: serde::de::DeserializeOwned>(Arc<Box<dyn CompletionProvider<D>>>);

impl<D: serde::de::DeserializeOwned> Agent<D> {
    pub fn agent(&self) -> Arc<Box<dyn CompletionProvider<D>>> {
        self.0.clone()
    }
}
