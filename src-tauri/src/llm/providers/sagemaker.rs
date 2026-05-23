use aws_sdk_sagemaker::primitives::Blob;

use crate::{error::Error, llm::_CompletionProvider};

#[derive(Clone)]
/// A custom SageMaker AI client that wraps the AWS SDK for SageMaker.
pub struct _CustomSageMakerAI {
    sagemaker_client: aws_sdk_sagemakerruntime::Client,
    endpoint_name: String,
}

#[derive(serde::Serialize)]
struct _Prompt<'a> {
    inputs: &'a str,
}

impl _CustomSageMakerAI {
    /// Builds a new SageMaker client using the provided endpoint name.
    pub async fn _build_sagemaker_client(endpoint_name: &str) -> Self {
        let config = aws_config::load_from_env().await;
        let sagemaker_client = aws_sdk_sagemakerruntime::Client::new(&config);

        Self {
            sagemaker_client,
            endpoint_name: endpoint_name.to_string(),
        }
    }

    /// Invokes the SageMaker endpoint with the given prompt and returns the response.
    async fn _invoke(&self, prompt: &str) -> Result<String, Error> {
        let payload = serde_json::to_vec(&_Prompt { inputs: prompt })?;

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
                if let aws_sdk_sagemakerruntime::error::SdkError::ServiceError(ref err) = e {
                    let body_str =
                        std::str::from_utf8(err.raw().body().bytes().unwrap_or_default());

                    tracing::warn!("SageMaker error body: {:?}", body_str);
                }

                Err(Error::Invoke(Box::new(e)))
            }
        }
    }
}

#[async_trait::async_trait]
impl _CompletionProvider for _CustomSageMakerAI {
    #[tracing::instrument(name = "sagemaker.chat", skip(self, prompt))]
    async fn chat(&self, prompt: &str) -> Result<String, Error> {
        self._invoke(prompt).await
    }
}
