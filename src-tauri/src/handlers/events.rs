use tauri::{AppHandle, Emitter as _, State};
use tokio::time::{self, Duration};

use crate::error::Error;
use crate::{InboxPopulateUpdateState, InitialDbPopulation, SentPopulateUpdateState};

#[tauri::command]
pub async fn inbox_email_populated(
    app: AppHandle,
    inbox_progress_state: tauri::State<'_, InboxPopulateUpdateState>,
) -> Result<(), Error> {
    let mut ticker = time::interval(Duration::from_millis(100));

    loop {
        let progress = inbox_progress_state
            .0
            .lock()
            .await
            .load(std::sync::atomic::Ordering::Acquire);

        app.emit("inbox-population-progress", progress)?;

        if progress >= 100 {
            break;
        }

        ticker.tick().await;
    }

    Ok(())
}

#[tauri::command]
pub async fn sent_email_populated(
    app: AppHandle,
    sent_progress_state: tauri::State<'_, SentPopulateUpdateState>,
) -> Result<(), Error> {
    let mut ticker = time::interval(Duration::from_millis(100));

    loop {
        let progress = sent_progress_state
            .0
            .lock()
            .await
            .load(std::sync::atomic::Ordering::Acquire);

        app.emit("sent-population-progress", progress)?;

        if progress >= 100 {
            break;
        }

        ticker.tick().await;
    }

    Ok(())
}

#[tauri::command]
pub async fn inbox_intial_email_populated(
    app: AppHandle,
    state: State<'_, InitialDbPopulation>,
) -> Result<(), Error> {
    let mut lock = state.inbox_rx.lock().await;

    if let Some(rx) = lock.take() {
        if let Ok(()) = rx.await {
            tracing::info!("Inbox initial population complete");
            app.emit("inbox-initial-population-complete", ())?;
        }
    } else {
        tracing::warn!("Inbox receiver was already consumed or is missing");
    }

    Ok(())
}

#[tauri::command]
pub async fn sent_intial_email_populated(
    app: AppHandle,
    state: State<'_, InitialDbPopulation>,
) -> Result<(), Error> {
    let mut lock = state.sent_rx.lock().await;

    if let Some(rx) = lock.take() {
        if let Ok(()) = rx.await {
            tracing::info!("Sent initial population complete");
            app.emit("sent-initial-population-complete", ())?;
        }
    } else {
        tracing::warn!("Sent receiver was already consumed or is missing");
    }

    Ok(())
}
