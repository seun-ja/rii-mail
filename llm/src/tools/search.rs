use crate::error::Error;
use rig::tool::Tool;
use schemars::{JsonSchema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use rig::completion::ToolDefinition;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchEmailsArgs {
    #[schemars(description = "Natural language search query")]
    pub query: String,

    #[schemars(description = "Maximum number of emails to return")]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct EmailSummary {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub received_at: String,
    pub snippet: String,
    pub unread: bool,
}

pub struct SearchEmailsTool;

impl SearchEmailsTool {
    pub async fn search(_query: String, _limit: usize) -> Result<Vec<EmailSummary>, Error> {
        todo!("query db")
    }
}

impl Tool for SearchEmailsTool {
    const NAME: &'static str = "search_emails";

    type Error = Error;
    type Args = SearchEmailsArgs;
    type Output = Vec<EmailSummary>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search emails in the user's mailbox using natural language.".to_string(),
            parameters: serde_json::to_value(SearchEmailsArgs::json_schema(
                &mut SchemaGenerator::default(),
            ))
            .unwrap(),
        }
    }

    async fn call(&self, args: SearchEmailsArgs) -> Result<Self::Output, Self::Error> {
        Self::search(args.query, args.limit.unwrap_or(20)).await
    }
}
