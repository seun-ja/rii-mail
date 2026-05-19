use tauri::Manager as _;

use crate::{
    config::{AppState, Config},
    error::Error,
    initialize_services,
};

#[tauri::command]
#[tracing::instrument(name = "command.config.setup", skip(app, config))]
pub async fn config_setup(app: tauri::AppHandle, config: Config) -> Result<(), Error> {
    let config_json = serde_json::to_string_pretty(&config)?;

    let path = app.path().app_config_dir()?.join("config.json");
    std::fs::write(path, config_json)?;

    initialize_services(app, config).await?;

    Ok(())
}

#[tauri::command]
#[tracing::instrument(name = "command.config.check", skip(state))]
pub async fn is_initialized(state: tauri::State<'_, AppState>) -> Result<bool, Error> {
    Ok(state.inner.read().await.is_some())
}
