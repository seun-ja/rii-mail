use arc_swap::ArcSwap;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter as _, Manager as _};

use tokio::sync::mpsc;

use crate::config::ImapClientConfig;
use crate::imap::ImapCommand;
use crate::workers::session_thread;
use crate::{
    config::AppState,
    handlers::{
        check_init_status, config_setup, login, logout, logout_with_state, open_main_window, rater,
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

    let (imap_cmd_channel_tx, imap_cmd_channel_rx) = mpsc::unbounded_channel::<ImapCommand>();

    session_thread(imap_client_channel_rx, imap_cmd_channel_rx);

    tauri::Builder::default()
        .manage(ArcSwap::from_pointee(AppState::Fresh))
        .manage(imap_client_channel_tx)
        .manage(imap_cmd_channel_tx)
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
                }

                if let Some(window) = app_handle.get_webview_window("main-app") {
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
            login,
            logout,
            open_main_window,
            rater,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
