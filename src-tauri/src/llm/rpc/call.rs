use crate::{
    error::{CrashAction, inference_crash_handler, is_connection_error}, llm::{SpamRating, rpc::{AgentWorkerClient, MessageType}},
};
use serde::Serialize;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use rpc_agent::error::{ApiError, Error as RpcError};
use tarpc::context;

use crate::{error::Error};

const MAX_RETRIES: u32 = 3;

#[tracing::instrument(name = "rpc.caller", skip(rpc_client, email))]
pub async fn caller(
    rpc_client: &AgentWorkerClient,
    email: impl Sized + Serialize + MessageType,
) -> Result<SpamRating, Error> {
    let mut ctx: context::Context = context::current();
    ctx.deadline = Instant::now() + Duration::from_secs(120);

    for attempt in 0..=MAX_RETRIES {
        let result: Result<String, ApiError> = rpc_client
            .message(ctx, email.msg_type()?)
            .await
            .map_err(RpcError::RpcError)
            .map_err(ApiError::from)
            .and_then(|inner| inner);

        match result {
            Ok(res) => return res.try_into(),

            Err(e) => {
                let err = Error::from(e);

                // connection-level retry (network, timeout, etc.)
                if is_connection_error(&err) && attempt < MAX_RETRIES {
                    let backoff = 100 * (attempt + 1) as u64;

                    tracing::warn!(attempt, backoff, "Connection error, retrying");

                    sleep(Duration::from_millis(backoff)).await;
                    continue;
                }

                // inference-level retry (MODEL_OOM etc.)
                match inference_crash_handler(&err.to_string(), attempt) {
                    CrashAction::Retry { backoff_ms, reason } if attempt < MAX_RETRIES => {
                        tracing::warn!(
                            error = %err,
                            attempt,
                            backoff_ms,
                            reason,
                            "Inference error, retrying"
                        );
                        sleep(Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    CrashAction::Fatal => return Err(Error::NotFound),
                    CrashAction::Ignore => return Err(err),
                    _ => return Err(err),
                }
            }
        }
    }

    Err(Error::Other("RPC call failed after retries".into()))
}