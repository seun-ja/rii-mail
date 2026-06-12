use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::Display;

use crate::{error::Error, rpc_llm::rpc::MessageType};

mod rpc;

pub use rpc::{caller, AgentWorkerClient};

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

impl MessageType for Email {
    fn msg_type(&self) -> Result<rpc_agent::Message, Error> {
        let message = serde_json::to_value(self).map_err(|e| Error::Other(e.to_string()))?;

        Ok(rpc_agent::Message::Struct(message))
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
            response[0].clone()
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
