use arc_swap::ArcSwap;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter as _, LogicalSize, Manager as _, Size};

use tokio::sync::{mpsc, Mutex};

use crate::config::{ImapClientConfig, ReturningUserImapClientConfig};
use crate::handlers::fetch_emails_handler;
use crate::imap::ImapCommand;
use crate::workers::session_thread;
use crate::{
    config::AppState,
    handlers::{
        check_init_status, config_setup, login, logout_with_state, open_main_window, rater,
    },
};

pub mod auth;
mod config;
pub mod db;
pub mod email_cache;
mod email_providers;
mod error;
mod handlers;
mod imap;
mod llm;
mod tracing;
mod workers;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let (imap_client_channel_tx, imap_client_channel_rx) =
        mpsc::unbounded_channel::<ImapClientConfig>();

    let (imap_client_returning_user_channel_tx, imap_client_returning_user_channel_rx) =
        mpsc::unbounded_channel::<ReturningUserImapClientConfig>();

    let (imap_cmd_channel_tx, imap_cmd_channel_rx) = mpsc::unbounded_channel::<ImapCommand>();

    let (logout_state_tx, logout_state_rx) = mpsc::unbounded_channel::<()>();

    session_thread(
        imap_client_channel_rx,
        imap_client_returning_user_channel_rx,
        imap_cmd_channel_rx,
        logout_state_tx,
    );

    tauri::Builder::default()
        .manage(ArcSwap::from_pointee(AppState::Fresh))
        .manage(imap_client_channel_tx)
        .manage(imap_client_returning_user_channel_tx)
        .manage(imap_cmd_channel_tx)
        .manage(Mutex::new(logout_state_rx))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_about = PredefinedMenuItem::about(app, None, None)?;
            let app_quit = PredefinedMenuItem::quit(app, None)?;
            let app_separator = PredefinedMenuItem::separator(app)?;
            let app_submenu = Submenu::with_items(
                app,
                "PhisherMan",
                true,
                &[&app_about, &app_separator, &app_quit],
            )?;

            let edit_undo = PredefinedMenuItem::undo(app, None)?;
            let edit_redo = PredefinedMenuItem::redo(app, None)?;
            let edit_separator_1 = PredefinedMenuItem::separator(app)?;
            let edit_cut = PredefinedMenuItem::cut(app, None)?;
            let edit_copy = PredefinedMenuItem::copy(app, None)?;
            let edit_paste = PredefinedMenuItem::paste(app, None)?;
            let edit_select_all = PredefinedMenuItem::select_all(app, None)?;
            let edit_submenu = Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &edit_undo,
                    &edit_redo,
                    &edit_separator_1,
                    &edit_cut,
                    &edit_copy,
                    &edit_paste,
                    &edit_select_all,
                ],
            )?;

            let logout_item =
                MenuItem::with_id(app, "logout", "Logout", true, Some("Cmd+Shift+L"))?;
            let account_submenu = Submenu::with_items(app, "Account", true, &[&logout_item])?;

            let menu = Menu::with_items(app, &[&app_submenu, &edit_submenu, &account_submenu])?;

            app.set_menu(menu)?;

            // On app relaunch for an already authenticated user, immediately expand
            // the default startup window to main-app dimensions.
            if let Some(window) = app.get_webview_window("main") {
                let config_dir = app.path().app_config_dir()?;

                if config_dir.join("data").exists() {
                    let expanded_size = Size::Logical(LogicalSize::new(1100.0, 760.0));
                    let expanded_min_size = Size::Logical(LogicalSize::new(900.0, 620.0));

                    window.set_resizable(true)?;
                    window.set_min_size(Some(expanded_min_size))?;
                    window.set_size(expanded_size)?;
                }
            }

            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() != "logout" {
                return;
            }

            let app_handle = app.clone();

            tauri::async_runtime::spawn(async move {
                if let Err(err) = logout_with_state(app_handle.clone()).await {
                    ::tracing::warn!(error = ?err, "Menu logout failed");
                } else {
                    ::tracing::info!("Logout succeeded");
                }

                let target_window = app_handle
                    .get_webview_window("main-app")
                    .or_else(|| app_handle.get_webview_window("main"));

                if let Some(window) = target_window {
                    let compact_size = Size::Logical(LogicalSize::new(500.0, 700.0));

                    if let Err(err) = window.set_max_size::<Size>(None) {
                        ::tracing::warn!(error = ?err, "Failed to reset compact window max size");
                    }

                    if let Err(err) = window.set_min_size(Some(compact_size)) {
                        ::tracing::warn!(error = ?err, "Failed to set compact window min size");
                    }

                    if let Err(err) = window.set_size(compact_size) {
                        ::tracing::warn!(error = ?err, "Failed to set compact window size");
                    }

                    if let Err(err) = window.set_resizable(false) {
                        ::tracing::warn!(error = ?err, "Failed to set compact window resizable state");
                    }

                    if let Err(err) = window.eval("window.location.replace('/setup.html')") {
                        ::tracing::warn!(error = ?err, "Failed to redirect main window after logout");
                    }
                }

                if let Err(err) = app_handle.emit("app://logged-out", ()) {
                    ::tracing::warn!(error = ?err, "Failed to notify frontend after logout");
                }
            });
        })
        .invoke_handler(tauri::generate_handler![
            check_init_status,
            config_setup,
            fetch_emails_handler,
            login,
            open_main_window,
            rater,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
