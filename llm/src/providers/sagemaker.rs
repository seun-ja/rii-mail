use aws_sdk_sagemaker::primitives::Blob;
use schemars::JsonSchema;

use crate::{error::Error, providers::CompletionProvider};

#[derive(Clone)]
/// A custom SageMaker AI client that wraps the AWS SDK for SageMaker.
pub struct CustomSageMakerAI {
    sagemaker_client: aws_sdk_sagemakerruntime::Client,
    endpoint_name: String,
}

#[derive(serde::Serialize)]
struct Prompt<'a> {
    inputs: &'a str,
}

impl CustomSageMakerAI {
    /// Builds a new SageMaker client using the provided endpoint name.
    pub async fn build_sagemaker_client(endpoint_name: &str) -> Self {
        let config = aws_config::load_from_env().await;
        let sagemaker_client = aws_sdk_sagemakerruntime::Client::new(&config);

        Self {
            sagemaker_client,
            endpoint_name: endpoint_name.to_string(),
        }
    }

    /// Invokes the SageMaker endpoint with the given prompt and returns the response.
    async fn invoke(&self, prompt: &str) -> Result<String, Error> {
        let payload = serde_json::to_vec(&Prompt { inputs: prompt })?;

        let resp = self
            .sagemaker_client
            .invoke_endpoint()
            .endpoint_name(&self.endpoint_name)
            .content_type("application/json")
            .body(Blob::new(payload))
            .send()
            .await;

        match resp {
            Ok(resp) => {
                let result = resp.body.unwrap_or_default().into_inner();
                let response = String::from_utf8(result).unwrap_or_default();
                Ok(response)
            }
            Err(e) => {
                #[cfg(feature = "tracing")]
                if let aws_sdk_sagemakerruntime::error::SdkError::ServiceError(ref err) = e {
                    let body_str =
                        std::str::from_utf8(err.raw().body().bytes().unwrap_or_default());

                    tracing::warn!("SageMaker error body: {:?}", body_str);
                }

                Err(Error::InvokeError(Box::new(e)))
            }
        }
    }
}

#[async_trait::async_trait]
impl<J: JsonSchema + serde::de::DeserializeOwned> CompletionProvider<J> for CustomSageMakerAI {
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "sagemaker.chat", skip(self, prompt))
    )]
    async fn chat(&self, prompt: &str) -> Result<String, Error> {
        self.invoke(prompt).await
    }

    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "sagemaker.schema_chat", skip(self, prompt))
    )]
    async fn schema_chat(&self, prompt: &str) -> Result<J, Error> {
        let response = self.invoke(prompt).await?;
        let res = serde_json::from_str(&response)?;
        Ok(res)
    }
}
