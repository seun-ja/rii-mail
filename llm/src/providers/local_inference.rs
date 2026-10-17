use std::{
    env,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use pyo3::prelude::*;
use pyo3::{
    exceptions::{PyImportError, PyRuntimeError},
    types::{PyDict, PyList, PyModule},
    Py, PyAny, PyResult, Python,
};
use schemars::JsonSchema;

use crate::{error::Error, providers::CompletionProvider};

static PYTHON_INIT_LOCK: Mutex<()> = Mutex::new(());
static PYTHON_RESOURCE_ROOT: OnceLock<PathBuf> = OnceLock::new();

#[derive(Clone)]
/// A local inference client backed by the Python runtime bundled with the app.
pub struct LocalInferenceAI {
    pub local_model: LocalModel,
}

impl LocalInferenceAI {
    /// Sets up local inference using the bundled Python runtime next to the script and model.
    pub fn setup(script_path: &str, function: &str, model_path: &str) -> Result<Self, Error> {
        Ok(Self {
            local_model: LocalModel::new(script_path, function, model_path)?,
        })
    }

    /// Invokes the local inference model with the given prompt and returns the response.
    fn invoke(&self, prompt: &str) -> Result<String, Error> {
        let result: PyResult<LocalInferenceResult> = Python::with_gil(|py| {
            let func = self.local_model.predict_fn.bind(py);
            let inference = func.call1((prompt,))?;

            let result_dict = inference.downcast::<PyDict>()?;
            let label: String = result_dict
                .get_item("label")?
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing label"))?
                .extract()?;
            let score: f64 = result_dict
                .get_item("score")?
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing score"))?
                .extract()?;

            Ok(LocalInferenceResult { label, score })
        });

        Ok(String::try_from(result?)?)
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
    pub fn new(script_path: &str, function: &str, model_path: &str) -> Result<Self, Error> {
        let script_path = Path::new(script_path).canonicalize()?;
        let model_path = Path::new(model_path).canonicalize()?;
        let paths = BundledPythonPaths::from_script(&script_path)?;
        initialize_bundled_python(&paths)?;

        let script_directory = script_path
            .parent()
            .ok_or_else(|| Error::Other(format!("Python script has no parent: {script_path:?}")))?;
        let module_name = script_path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Other(format!("invalid Python script name: {script_path:?}")))?;

        Python::with_gil(|py| -> PyResult<Self> {
            configure_sys_path(py, &paths, script_directory)?;
            assert_bundled_dependency(py, "torch", &paths.site_packages)?;
            assert_bundled_dependency(py, "transformers", &paths.site_packages)?;

            let module = PyModule::import_bound(py, module_name)?;
            module.setattr("MODEL_DIR", path_as_utf8(&model_path, "model")?)?;
            let func = module.getattr(function)?;

            Ok(Self {
                predict_fn: func.into_py(py),
            })
        })
        .map_err(Error::from)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BundledPythonPaths {
    resource_root: PathBuf,
    python_home: PathBuf,
    python_executable: PathBuf,
    stdlib: PathBuf,
    lib_dynload: PathBuf,
    site_packages: PathBuf,
}

impl BundledPythonPaths {
    fn from_script(script_path: &Path) -> Result<Self, Error> {
        let resource_root = script_path.parent().and_then(Path::parent).ok_or_else(|| {
            Error::Other(format!(
                "bundled Python script is not inside <Resources>/llm: {script_path:?}"
            ))
        })?;
        Self::from_resource_root(resource_root)
    }

    fn from_resource_root(resource_root: &Path) -> Result<Self, Error> {
        let resource_root = resource_root.canonicalize()?;
        let python_home = require_directory(&resource_root.join("python"), "Python home")?;
        let python_executable =
            require_file(&python_home.join("bin/python3.12"), "Python executable")?;
        let stdlib = require_directory(
            &python_home.join("lib/python3.12"),
            "Python standard library",
        )?;
        let lib_dynload = require_directory(&stdlib.join("lib-dynload"), "Python lib-dynload")?;
        let site_packages =
            require_directory(&stdlib.join("site-packages"), "Python site-packages")?;
        require_directory(&site_packages.join("torch"), "bundled torch package")?;
        require_directory(
            &site_packages.join("transformers"),
            "bundled transformers package",
        )?;

        Ok(Self {
            resource_root,
            python_home,
            python_executable,
            stdlib,
            lib_dynload,
            site_packages,
        })
    }
}

fn require_file(path: &Path, description: &str) -> Result<PathBuf, Error> {
    if !path.is_file() {
        return Err(Error::Other(format!(
            "{description} is missing from the app bundle: {}",
            path.display()
        )));
    }
    Ok(path.canonicalize()?)
}

fn require_directory(path: &Path, description: &str) -> Result<PathBuf, Error> {
    if !path.is_dir() {
        return Err(Error::Other(format!(
            "{description} is missing from the app bundle: {}",
            path.display()
        )));
    }
    Ok(path.canonicalize()?)
}

fn initialize_bundled_python(paths: &BundledPythonPaths) -> Result<(), Error> {
    let _guard = PYTHON_INIT_LOCK
        .lock()
        .map_err(|_| Error::Other("bundled Python initialization lock is poisoned".to_string()))?;

    if let Some(initialized_root) = PYTHON_RESOURCE_ROOT.get() {
        if initialized_root != &paths.resource_root {
            return Err(Error::Other(format!(
                "Python was already initialized from {}, refusing resource root {}",
                initialized_root.display(),
                paths.resource_root.display()
            )));
        }
        return Ok(());
    }

    for name in [
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONUSERBASE",
        "PYTHONSTARTUP",
        "PYTHONEXECUTABLE",
        "PYTHONINSPECT",
        "VIRTUAL_ENV",
        "CONDA_PREFIX",
        "_CE_CONDA",
        "_CE_M",
    ] {
        env::remove_var(name);
    }

    env::set_var("PYTHONHOME", &paths.python_home);
    env::set_var("PYTHONPATH", &paths.site_packages);
    env::set_var("PYTHONNOUSERSITE", "1");
    env::set_var("PYTHONSAFEPATH", "1");
    env::set_var("PYTHONDONTWRITEBYTECODE", "1");
    env::set_var("HF_HUB_OFFLINE", "1");
    env::set_var("TRANSFORMERS_OFFLINE", "1");
    env::set_var("TOKENIZERS_PARALLELISM", "false");
    env::set_var("RIIMAIL_BUNDLED_PYTHON_ROOT", &paths.resource_root);

    pyo3::prepare_freethreaded_python();

    Python::with_gil(|py| -> PyResult<()> {
        configure_sys_path(py, paths, &paths.resource_root.join("llm"))?;
        configure_python_executable(py, &paths.python_executable)?;
        let sys = PyModule::import_bound(py, "sys")?;
        let prefix: PathBuf = PathBuf::from(sys.getattr("prefix")?.extract::<String>()?);
        let prefix = prefix.canonicalize().map_err(|error| {
            PyErr::new::<PyRuntimeError, _>(format!(
                "embedded Python reported invalid sys.prefix {}: {error}",
                prefix.display()
            ))
        })?;
        if prefix != paths.python_home {
            return Err(PyErr::new::<PyRuntimeError, _>(format!(
                "embedded Python escaped the app bundle: sys.prefix={} expected={}",
                prefix.display(),
                paths.python_home.display()
            )));
        }
        Ok(())
    })?;

    PYTHON_RESOURCE_ROOT
        .set(paths.resource_root.clone())
        .map_err(|_| Error::Other("bundled Python initialization raced".to_string()))?;
    Ok(())
}

fn configure_sys_path(
    py: Python<'_>,
    paths: &BundledPythonPaths,
    script_directory: &Path,
) -> PyResult<()> {
    let sys = PyModule::import_bound(py, "sys")?;
    let sys_path = PyList::empty_bound(py);
    for (path, description) in [
        (paths.stdlib.as_path(), "Python standard library"),
        (paths.lib_dynload.as_path(), "Python lib-dynload"),
        (paths.site_packages.as_path(), "Python site-packages"),
        (script_directory, "Python script directory"),
    ] {
        sys_path.append(path_as_utf8(path, description)?)?;
    }
    sys.setattr("path", &sys_path)?;
    sys.setattr("dont_write_bytecode", true)?;
    Ok(())
}

fn configure_python_executable(py: Python<'_>, executable: &Path) -> PyResult<()> {
    let executable = path_as_utf8(executable, "Python executable")?;
    let sys = PyModule::import_bound(py, "sys")?;
    sys.setattr("executable", executable)?;
    sys.setattr("_base_executable", executable)?;

    let multiprocessing = PyModule::import_bound(py, "multiprocessing")?;
    multiprocessing
        .getattr("set_executable")?
        .call1((executable,))?;
    Ok(())
}

fn assert_bundled_dependency(
    py: Python<'_>,
    module_name: &str,
    site_packages: &Path,
) -> PyResult<()> {
    let module = PyModule::import_bound(py, module_name).map_err(|error| {
        PyErr::new::<PyImportError, _>(format!(
            "failed to import bundled {module_name} from {}: {error}",
            site_packages.display()
        ))
    })?;
    let origin = module
        .getattr("__file__")?
        .extract::<String>()
        .map(PathBuf::from)?;
    let origin = origin.canonicalize().map_err(|error| {
        PyErr::new::<PyImportError, _>(format!(
            "{module_name} reported invalid module origin {}: {error}",
            origin.display()
        ))
    })?;

    if !origin.starts_with(site_packages) {
        return Err(PyErr::new::<PyImportError, _>(format!(
            "refusing external {module_name} module at {}; expected it under {}",
            origin.display(),
            site_packages.display()
        )));
    }
    Ok(())
}

fn path_as_utf8<'a>(path: &'a Path, description: &str) -> PyResult<&'a str> {
    path.to_str().ok_or_else(|| {
        PyErr::new::<PyRuntimeError, _>(format!("{description} path is not valid UTF-8"))
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after the Unix epoch")
            .as_nanos();
        env::temp_dir().join(format!("riimail-llm-{name}-{nanos}"))
    }

    fn create_bundle_fixture(root: &Path) -> PathBuf {
        let script = root.join("llm/local_inference.py");
        fs::create_dir_all(script.parent().expect("script should have a parent"))
            .expect("script directory should be created");
        fs::write(&script, "def predict(_text): return {}\n")
            .expect("fixture script should be written");
        let python_bin = root.join("python/bin");
        fs::create_dir_all(&python_bin).expect("Python bin should be created");
        fs::write(python_bin.join("python3.12"), "fixture")
            .expect("Python executable should be created");
        let stdlib = root.join("python/lib/python3.12");
        fs::create_dir_all(stdlib.join("lib-dynload")).expect("lib-dynload should be created");
        fs::create_dir_all(stdlib.join("site-packages/torch")).expect("torch should be created");
        fs::create_dir_all(stdlib.join("site-packages/transformers"))
            .expect("transformers should be created");
        script
    }

    #[test]
    fn local_inference_result_serializes_expected_json() {
        let result = String::try_from(LocalInferenceResult {
            label: "LABEL_1".to_string(),
            score: 0.875,
        })
        .expect("result should serialize");

        assert_eq!(result, r#"{"label":"LABEL_1","score":0.875}"#);
    }

    #[test]
    fn bundled_paths_are_derived_from_the_script_resource_root() {
        let root = unique_test_dir("bundle-paths");
        let script = create_bundle_fixture(&root);

        let paths = BundledPythonPaths::from_script(&script).expect("bundle should validate");

        assert_eq!(paths.resource_root, root.canonicalize().unwrap());
        assert_eq!(
            paths.python_home,
            root.join("python").canonicalize().unwrap()
        );
        assert_eq!(
            paths.site_packages,
            root.join("python/lib/python3.12/site-packages")
                .canonicalize()
                .unwrap()
        );
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn bundled_paths_reject_missing_runtime() {
        let root = unique_test_dir("missing-runtime");
        let script = root.join("llm/local_inference.py");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, "").unwrap();

        let error = BundledPythonPaths::from_script(&script).expect_err("runtime must be required");

        assert!(error.to_string().contains("Python home is missing"));
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn local_model_reports_missing_script_without_initializing_python() {
        let missing = unique_test_dir("missing-script").join("missing.py");
        let error = match LocalModel::new(
            missing.to_str().expect("path should be UTF-8"),
            "predict",
            "/tmp/model",
        ) {
            Ok(_) => panic!("missing scripts should return an error"),
            Err(error) => error,
        };

        assert!(matches!(error, Error::Io(_)));
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
