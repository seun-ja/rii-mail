use pyo3::prelude::*;
use pyo3::{
    types::{PyDict, PyModule},
    Py, PyAny, PyResult, Python,
};
use schemars::JsonSchema;

use crate::{error::Error, providers::CompletionProvider};

#[derive(Clone)]
/// A custom SageMaker AI client that wraps the AWS SDK for SageMaker.
pub struct LocalInferenceAI {
    pub local_model: LocalModel,
}

impl LocalInferenceAI {
    /// Sets up a new LocalInferenceAI client using the provided model directory.
    pub fn setup(script_name: String, function: String) -> Self {
        Self {
            local_model: LocalModel::new(&script_name, &function).unwrap(),
        }
    }

    /// Invokes the local inference model with the given prompt and returns the response.
    fn invoke(&self, prompt: &str) -> Result<String, Error> {
        let p: PyResult<LocalInferenceResult> = Python::with_gil(|py| {
            let func = self.local_model.predict_fn.bind(py);
            let inference = func.call1((prompt,))?;

            let result_dict = inference.downcast::<PyDict>()?;
            let label: String = result_dict
                .get_item("label")?
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing label"))?
                .extract()?;
            let score: f64 = result_dict
                .get_item("score")?
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing label"))?
                .extract()?;

            Ok(LocalInferenceResult { label, score })
        });

        let res: String = p?.try_into()?;

        Ok(res)
    }
}

#[derive(serde::Serialize)]
struct LocalInferenceResult {
    label: String,
    score: f64,
}

impl TryFrom<LocalInferenceResult> for String {
    type Error = Error;

    fn try_from(value: LocalInferenceResult) -> Result<Self, Self::Error> {
        serde_json::to_string(&value).map_err(Self::Error::SerializationError)
    }
}

#[derive(Clone)]
pub struct LocalModel {
    predict_fn: Py<PyAny>,
}

impl LocalModel {
    pub fn new(script_name: &str, function: &str) -> PyResult<Self> {
        Python::with_gil(|py| {
            let module = PyModule::import_bound(py, script_name)?;
            let func = module.getattr(function)?;

            Ok(Self {
                predict_fn: func.into_py(py),
            })
        })
    }
}

#[async_trait::async_trait]
impl<J: JsonSchema + serde::de::DeserializeOwned> CompletionProvider<J> for LocalInferenceAI {
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "local_inference.chat", skip(self, prompt))
    )]
    async fn chat(&self, prompt: &str) -> Result<String, Error> {
        self.invoke(prompt)
    }

    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(name = "local_inference.schema_chat", skip(self, prompt))
    )]
    async fn schema_chat(&self, _prompt: &str) -> Result<J, Error> {
        todo!()
    }
}
