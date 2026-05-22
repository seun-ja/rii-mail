use std::sync::Arc;

use crate::{config::AppState, db, error::Error};
use arc_swap::ArcSwap;
use tauri::Manager as _;
use tokio::fs;

#[tracing::instrument(name = "command.logout.menu", skip(app))]
pub async fn logout_with_state(app: tauri::AppHandle) -> Result<(), Error> {
    let config_dir = app.path().app_config_dir()?;
    let config_path = config_dir.join("config.json");

    if let Err(err) = fs::remove_file(config_path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err.into());
        }
    }

    let state = app.state::<ArcSwap<AppState>>();
    let sqlite_pool = {
        let current_state = state.load();
        match current_state.as_ref() {
            AppState::Initialized(initialized) => Some(initialized.sqlite_pool.clone()),
            AppState::Fresh => None,
        }
    };

    if let Some(pool) = sqlite_pool {
        db::cleanup(&pool).await?;
    }

    state.store(Arc::new(AppState::Fresh));

    Ok(())
}

#[tauri::command]
#[tracing::instrument(name = "command.logout", skip(app))]
pub async fn logout(app: tauri::AppHandle) -> Result<(), Error> {
    logout_with_state(app).await
}
