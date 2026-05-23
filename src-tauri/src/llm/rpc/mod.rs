use rpc_agent::{error::ApiError, Message};

use crate::error::Error;

mod call;

pub use call::caller;

#[tarpc::service]
pub trait AgentWorker {
    async fn message(user_message: Message) -> Result<String, ApiError>;
}

pub trait MessageType {
    fn msg_type(&self) -> Result<Message, Error>;
}