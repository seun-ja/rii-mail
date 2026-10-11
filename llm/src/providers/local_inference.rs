use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use pyo3::prelude::*;
use pyo3::{
    types::{PyDict, PyList, PyModule},
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
    /// Sets up a new LocalInferenceAI client using the provided Python script and model directory.
    pub fn setup(script_path: &str, function: &str, model_path: &str) -> Result<Self, Error> {
        Ok(Self {
            local_model: LocalModel::new(script_path, function, model_path)?,
        })
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
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing score"))?
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
    pub fn new(script_path: &str, function: &str, model_path: &str) -> Result<Self, Error> {
        let script_path = Path::new(script_path).canonicalize()?;
        let script_directory = script_path
            .parent()
            .and_then(Path::to_str)
            .ok_or_else(|| Error::Other(format!("invalid Python script path: {script_path:?}")))?;
        let module_name = script_path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Other(format!("invalid Python script name: {script_path:?}")))?;

        let project_root = script_path.parent().and_then(Path::parent);

        Python::with_gil(|py| -> PyResult<Self> {
            let python_environment = add_python_site_packages(py, project_root)?;

            if let Err(error) = PyModule::import_bound(py, "torch") {
                let sys = PyModule::import_bound(py, "sys")?;
                let sys_path = sys.getattr("path")?.repr()?.to_string();
                return Err(PyErr::new::<pyo3::exceptions::PyImportError, _>(format!(
                    "failed to import torch using Python environment {python_environment:?}; \
                     embedded sys.path={sys_path}; original error: {error}"
                )));
            }

            let sys = PyModule::import_bound(py, "sys")?;
            let sys_path = sys.getattr("path")?;
            let sys_path = sys_path.downcast::<PyList>()?;
            sys_path.insert(0, script_directory)?;

            let module = PyModule::import_bound(py, module_name)?;
            module.setattr("MODEL_DIR", model_path)?;
            let func = module.getattr(function)?;

            Ok(Self {
                predict_fn: func.into_py(py),
            })
        })
        .map_err(Error::from)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PythonEnvironment {
    executable: PathBuf,
    site_packages: PathBuf,
}

fn add_python_site_packages(
    py: Python<'_>,
    project_root: Option<&Path>,
) -> PyResult<Option<PythonEnvironment>> {
    let sys = PyModule::import_bound(py, "sys")?;
    let version = sys.getattr("version_info")?;
    let major: u8 = version.getattr("major")?.extract()?;
    let minor: u8 = version.getattr("minor")?.extract()?;
    let environment = discover_python_environment(project_root, major, minor);

    let Some(environment) = environment else {
        return Ok(None);
    };

    let site_packages = environment.site_packages.to_str().ok_or_else(|| {
        PyErr::new::<pyo3::exceptions::PyImportError, _>(
            "Python site-packages path is not valid UTF-8",
        )
    })?;
    let site = PyModule::import_bound(py, "site")?;
    site.call_method1("addsitedir", (site_packages,))?;

    let sys_path = sys.getattr("path")?;
    let sys_path = sys_path.downcast::<PyList>()?;
    sys_path.insert(0, site_packages)?;

    configure_python_executable(py, &sys, &environment.executable)?;

    Ok(Some(environment))
}

fn configure_python_executable(
    py: Python<'_>,
    sys: &Bound<'_, PyModule>,
    executable: &Path,
) -> PyResult<()> {
    let executable = executable.to_str().ok_or_else(|| {
        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "Python executable path is not valid UTF-8",
        )
    })?;

    sys.setattr("executable", executable)?;
    sys.setattr("_base_executable", executable)?;

    let multiprocessing = PyModule::import_bound(py, "multiprocessing")?;
    multiprocessing
        .getattr("set_executable")?
        .call1((executable,))?;

    Ok(())
}

fn discover_python_environment(
    project_root: Option<&Path>,
    major: u8,
    minor: u8,
) -> Option<PythonEnvironment> {
    python_executable_candidates(project_root, major, minor)
        .into_iter()
        .find_map(|executable| python_environment_from_executable(&executable, major, minor))
}

fn python_executable_candidates(
    project_root: Option<&Path>,
    major: u8,
    minor: u8,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
        push_unique(
            &mut candidates,
            virtual_env_python(&PathBuf::from(virtual_env)),
        );
    }

    if let Some(project_root) = project_root {
        for name in [".venv", ".venv312"] {
            push_unique(
                &mut candidates,
                virtual_env_python(&project_root.join(name)),
            );
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let environments_root = PathBuf::from(home).join(".venvs");
        if let Ok(environments) = fs::read_dir(environments_root) {
            let mut environments = environments
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            environments.sort();

            for environment in environments {
                push_unique(&mut candidates, virtual_env_python(&environment));
            }
        }
    }

    for executable in [
        format!("python{major}.{minor}"),
        "python3".to_string(),
        "python".to_string(),
    ] {
        push_unique(&mut candidates, PathBuf::from(executable));
    }

    candidates
}

fn push_unique(candidates: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn virtual_env_python(virtual_env: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    return virtual_env.join("Scripts").join("python.exe");

    #[cfg(not(target_os = "windows"))]
    virtual_env.join("bin").join("python")
}

fn python_environment_from_executable(
    executable: &Path,
    major: u8,
    minor: u8,
) -> Option<PythonEnvironment> {
    let output = Command::new(executable)
        .args([
            "-c",
            "import pathlib, sys, torch; print(f'{sys.version_info.major}.{sys.version_info.minor}'); print(pathlib.Path(sys.executable).resolve()); print(pathlib.Path(torch.__file__).resolve().parent.parent)",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let output = String::from_utf8(output.stdout).ok()?;
    let mut lines = output.lines();
    if lines.next()? != format!("{major}.{minor}") {
        return None;
    }

    let executable = PathBuf::from(lines.next()?);
    let site_packages = PathBuf::from(lines.next()?);
    if !executable.is_file() || !site_packages.join("torch").is_dir() {
        return None;
    }

    Some(PythonEnvironment {
        executable,
        site_packages,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        sync::{Mutex, MutexGuard},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvironmentRestore {
        values: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvironmentRestore {
        fn capture(names: &[&'static str]) -> Self {
            Self {
                values: names
                    .iter()
                    .map(|name| (*name, env::var_os(name)))
                    .collect(),
            }
        }
    }

    impl Drop for EnvironmentRestore {
        fn drop(&mut self) {
            for (name, value) in &self.values {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }

    fn lock_environment() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after the Unix epoch")
            .as_nanos();
        env::temp_dir().join(format!("riimail-llm-{name}-{nanos}"))
    }

    fn configure_isolated_environment(home: &Path) -> EnvironmentRestore {
        let restore = EnvironmentRestore::capture(&["VIRTUAL_ENV", "HOME"]);
        env::remove_var("VIRTUAL_ENV");
        env::set_var("HOME", home);
        restore
    }

    fn create_fixture_script(root: &Path, name: &str, source: &str) -> PathBuf {
        let script_directory = root.join("llm");
        fs::create_dir_all(&script_directory).expect("script directory should be created");
        let script_path = script_directory.join(format!("{name}.py"));
        fs::write(&script_path, source).expect("fixture script should be written");
        script_path
    }

    fn embedded_python_version() -> (u8, u8) {
        Python::with_gil(|py| {
            let sys = PyModule::import_bound(py, "sys").expect("sys should import");
            let version = sys
                .getattr("version_info")
                .expect("version_info should exist");
            (
                version
                    .getattr("major")
                    .expect("major should exist")
                    .extract()
                    .expect("major should be numeric"),
                version
                    .getattr("minor")
                    .expect("minor should exist")
                    .extract()
                    .expect("minor should be numeric"),
            )
        })
    }

    #[cfg(unix)]
    fn create_fake_python_environment(
        virtual_env: &Path,
        major: u8,
        minor: u8,
    ) -> PythonEnvironment {
        use std::os::unix::fs::PermissionsExt;

        let executable = virtual_env_python(virtual_env);
        let site_packages = virtual_env
            .join("lib")
            .join(format!("python{major}.{minor}"))
            .join("site-packages");
        fs::create_dir_all(site_packages.join("torch"))
            .expect("fake torch package should be created");
        fs::write(site_packages.join("torch/__init__.py"), "")
            .expect("fake torch module should be created");
        fs::create_dir_all(executable.parent().expect("executable should have a parent"))
            .expect("executable directory should be created");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '{}.{}\\n{}\\n{}\\n'\n",
                major,
                minor,
                executable.display(),
                site_packages.display()
            ),
        )
        .expect("fake Python executable should be written");
        let mut permissions = fs::metadata(&executable)
            .expect("executable metadata should be available")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions)
            .expect("fake Python should be executable");

        PythonEnvironment {
            executable,
            site_packages,
        }
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
    fn virtual_environment_python_path_uses_platform_layout() {
        let root = Path::new("virtual-environment");

        #[cfg(target_os = "windows")]
        assert_eq!(
            virtual_env_python(root),
            root.join("Scripts").join("python.exe")
        );

        #[cfg(not(target_os = "windows"))]
        assert_eq!(virtual_env_python(root), root.join("bin").join("python"));
    }

    #[test]
    fn executable_candidates_are_dynamic_and_prioritized() {
        let _lock = lock_environment();
        let root = unique_test_dir("candidate-order");
        let home = root.join("home");
        let _restore = configure_isolated_environment(&home);
        let active = root.join("active");
        env::set_var("VIRTUAL_ENV", &active);
        fs::create_dir_all(home.join(".venvs/z-last")).expect("home venv should be created");
        fs::create_dir_all(home.join(".venvs/a-first")).expect("home venv should be created");

        let candidates = python_executable_candidates(Some(&root), 3, 12);

        assert_eq!(candidates[0], virtual_env_python(&active));
        assert_eq!(candidates[1], virtual_env_python(&root.join(".venv")));
        assert_eq!(candidates[2], virtual_env_python(&root.join(".venv312")));
        assert_eq!(
            candidates[3],
            virtual_env_python(&home.join(".venvs/a-first"))
        );
        assert_eq!(
            candidates[4],
            virtual_env_python(&home.join(".venvs/z-last"))
        );
        assert!(candidates.contains(&PathBuf::from("python3.12")));
        assert!(candidates.contains(&PathBuf::from("python3")));
        assert!(candidates.contains(&PathBuf::from("python")));

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn executable_probe_accepts_matching_python_and_rejects_mismatch() {
        let root = unique_test_dir("executable-probe");
        let environment = create_fake_python_environment(&root.join("python"), 3, 11);

        assert_eq!(
            python_environment_from_executable(&environment.executable, 3, 12),
            None
        );
        assert_eq!(
            python_environment_from_executable(&environment.executable, 3, 11),
            Some(environment)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn discovery_skips_mismatched_active_python_for_matching_project_python() {
        let _lock = lock_environment();
        let root = unique_test_dir("discovery-version");
        let _restore = configure_isolated_environment(&root.join("home"));
        let active_root = root.join("active");
        create_fake_python_environment(&active_root, 3, 11);
        env::set_var("VIRTUAL_ENV", &active_root);
        let expected = create_fake_python_environment(&root.join(".venv"), 3, 12);

        assert_eq!(
            discover_python_environment(Some(&root), 3, 12),
            Some(expected)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn discovery_uses_first_sorted_compatible_home_environment() {
        let _lock = lock_environment();
        let root = unique_test_dir("discovery-home");
        let home = root.join("home");
        let _restore = configure_isolated_environment(&home);
        create_fake_python_environment(&home.join(".venvs/z-last"), 3, 12);
        let expected = create_fake_python_environment(&home.join(".venvs/a-first"), 3, 12);

        assert_eq!(discover_python_environment(None, 3, 12), Some(expected));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_model_reports_missing_script_without_panicking() {
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

    #[cfg(unix)]
    #[test]
    fn local_model_invokes_fixture_and_injects_model_path() {
        let _lock = lock_environment();
        let root = unique_test_dir("fixture-success");
        let _restore = configure_isolated_environment(&root.join("home"));
        let (major, minor) = embedded_python_version();
        let virtual_env = root.join("python");
        create_fake_python_environment(&virtual_env, major, minor);
        env::set_var("VIRTUAL_ENV", &virtual_env);
        let script = create_fixture_script(
            &root,
            "success_fixture",
            r#"
MODEL_DIR = None
def predict(text):
    return {"label": f"{MODEL_DIR}|{text}", "score": 0.875}
"#,
        );
        let model_path = root.join("model");
        let local_model = LocalModel::new(
            script.to_str().expect("script path should be UTF-8"),
            "predict",
            model_path.to_str().expect("model path should be UTF-8"),
        )
        .expect("fixture model should initialize");
        let provider = LocalInferenceAI { local_model };

        let response = provider
            .invoke("héllo\nworld")
            .expect("fixture should return a result");
        let parsed: serde_json::Value =
            serde_json::from_str(&response).expect("response should be JSON");

        assert_eq!(parsed["score"], 0.875);
        assert_eq!(
            parsed["label"],
            format!("{}|héllo\nworld", model_path.display())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn local_model_reports_missing_handler() {
        let _lock = lock_environment();
        let root = unique_test_dir("missing-handler");
        let _restore = configure_isolated_environment(&root.join("home"));
        let (major, minor) = embedded_python_version();
        let virtual_env = root.join("python");
        create_fake_python_environment(&virtual_env, major, minor);
        env::set_var("VIRTUAL_ENV", &virtual_env);
        let script = create_fixture_script(&root, "missing_handler_fixture", "VALUE = 1\n");

        let error = match LocalModel::new(
            script.to_str().expect("script path should be UTF-8"),
            "predict",
            "/tmp/model",
        ) {
            Ok(_) => panic!("missing handlers should return an error"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("predict"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn invoke_rejects_missing_or_invalid_result_fields() {
        let _lock = lock_environment();
        let root = unique_test_dir("invalid-results");
        let _restore = configure_isolated_environment(&root.join("home"));
        let (major, minor) = embedded_python_version();
        let virtual_env = root.join("python");
        create_fake_python_environment(&virtual_env, major, minor);
        env::set_var("VIRTUAL_ENV", &virtual_env);

        for (name, body, expected) in [
            (
                "missing_label_fixture",
                "def predict(_text):\n    return {\"score\": 0.5}\n",
                "missing label",
            ),
            (
                "missing_score_fixture",
                "def predict(_text):\n    return {\"label\": \"LABEL_0\"}\n",
                "missing score",
            ),
            (
                "invalid_score_fixture",
                "def predict(_text):\n    return {\"label\": \"LABEL_0\", \"score\": \"high\"}\n",
                "real number",
            ),
        ] {
            let script = create_fixture_script(&root, name, body);
            let local_model = LocalModel::new(
                script.to_str().expect("script path should be UTF-8"),
                "predict",
                "/tmp/model",
            )
            .expect("fixture model should initialize");
            let error = LocalInferenceAI { local_model }
                .invoke("input")
                .expect_err("invalid results should fail");

            assert!(
                error.to_string().contains(expected),
                "expected {error} to contain {expected}"
            );
        }

        let _ = fs::remove_dir_all(root);
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
