use std::sync::atomic::AtomicU8;
use std::sync::{Arc, LazyLock};

use arc_swap::ArcSwap;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter as _, LogicalSize, Manager as _, Size};

use tokio::sync::{mpsc, oneshot, Mutex};

use crate::config::ImapClientConfig;
use crate::handlers::{
    email_generator, inbox_email_populated, inbox_intial_email_populated, refresh_emails_handler,
    send_email, sent_email_populated, sent_intial_email_populated,
};
use crate::imap::ImapCommand;
use crate::workers::{worker, FetchManager};
use crate::{
    config::{AppState, StartupStatusCache},
    handlers::{
        check_app_status, config_setup, fetch_emails, login, logout_with_state, open_main_window,
        rater,
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
mod rpc_llm;
pub mod tracing;
mod workers;

pub struct ImapClientChannelTx(pub mpsc::UnboundedSender<ImapClientConfig>);
pub struct ReturningUserImapClientChannelTx(pub mpsc::UnboundedSender<ImapClientConfig>);

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct InboxPopulateUpdateState(pub Arc<Mutex<AtomicU8>>);

#[derive(Clone)]
pub struct SentPopulateUpdateState(pub Arc<Mutex<AtomicU8>>);

pub static LOGGED_IN: LazyLock<Arc<Mutex<bool>>> = LazyLock::new(|| Arc::new(Mutex::new(false)));
pub static LOGGING_IN: LazyLock<Arc<Mutex<bool>>> = LazyLock::new(|| Arc::new(Mutex::new(false)));
pub static FETCH_MANAGER: LazyLock<Arc<FetchManager>> =
    LazyLock::new(|| Arc::new(FetchManager::default()));
pub static INBOX_POPULATE_UPDATE: LazyLock<InboxPopulateUpdateState> =
    LazyLock::new(|| InboxPopulateUpdateState(Arc::new(Mutex::new(AtomicU8::new(0)))));
pub static SENT_POPULATE_UPDATE: LazyLock<SentPopulateUpdateState> =
    LazyLock::new(|| SentPopulateUpdateState(Arc::new(Mutex::new(AtomicU8::new(0)))));

pub struct InitialDbPopulation {
    inbox_rx: Mutex<Option<oneshot::Receiver<()>>>,
    sent_rx: Mutex<Option<oneshot::Receiver<()>>>,
}

impl InitialDbPopulation {
    fn new(
        inbox_intial_email_populated_rx: oneshot::Receiver<()>,
        sent_intial_email_populated_rx: oneshot::Receiver<()>,
    ) -> Self {
        Self {
            inbox_rx: Mutex::new(Some(inbox_intial_email_populated_rx)),
            sent_rx: Mutex::new(Some(sent_intial_email_populated_rx)),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let (imap_client_channel_tx, imap_client_channel_rx) =
        mpsc::unbounded_channel::<ImapClientConfig>();

    let (imap_client_returning_user_channel_tx, imap_client_returning_user_channel_rx) =
        mpsc::unbounded_channel::<ImapClientConfig>();

    let (imap_cmd_channel_tx, imap_cmd_channel_rx) = mpsc::unbounded_channel::<ImapCommand>();

    let (logout_state_tx, logout_state_rx) = mpsc::unbounded_channel::<()>();

    let (inbox_intial_email_populated_tx, inbox_intial_email_populated_rx) =
        oneshot::channel::<()>();

    let (sent_intial_email_populated_tx, sent_intial_email_populated_rx) = oneshot::channel::<()>();

    worker(
        imap_client_channel_rx,
        imap_client_returning_user_channel_rx,
        imap_cmd_channel_rx,
        logout_state_tx,
        Arc::new(Mutex::new(Some(inbox_intial_email_populated_tx))),
        Arc::new(Mutex::new(Some(sent_intial_email_populated_tx))),
    )
    .await;

    tauri::Builder::default()
        .manage(ArcSwap::from_pointee(AppState::Fresh))
        .manage(StartupStatusCache(Mutex::new(None)))
        .manage(ImapClientChannelTx(imap_client_channel_tx))
        .manage(ReturningUserImapClientChannelTx(
            imap_client_returning_user_channel_tx,
        ))
        .manage(imap_cmd_channel_tx)
        .manage(Mutex::new(logout_state_rx))
        .manage(FETCH_MANAGER.clone())
        .manage(INBOX_POPULATE_UPDATE.clone())
        .manage(SENT_POPULATE_UPDATE.clone())
        .manage(InitialDbPopulation::new(inbox_intial_email_populated_rx, sent_intial_email_populated_rx))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .setup(|app| {
            let app_about = PredefinedMenuItem::about(app, None, None)?;
            let app_quit = PredefinedMenuItem::quit(app, None)?;
            let app_separator = PredefinedMenuItem::separator(app)?;
            let app_submenu = Submenu::with_items(
                app,
                "RiiMail",
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
                logout_with_state(app_handle.clone()).await;
                ::tracing::info!("Logout succeeded");


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
            check_app_status,
            email_generator,
            inbox_email_populated,
            inbox_intial_email_populated,
            config_setup,
            fetch_emails,
            login,
            open_main_window,
            rater,
            refresh_emails_handler,
            send_email,
            sent_email_populated,
            sent_intial_email_populated,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
