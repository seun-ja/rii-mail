use tauri::{AppHandle, Emitter as _, Manager};
use tokio::time::{self, Duration};

use crate::error::Error;
use crate::{InboxPopulateUpdateState, SentPopulateUpdateState};

#[tauri::command]
pub async fn inbox_email_populated(app: AppHandle) -> Result<(), Error> {
    let inbox_progress_state = app.state::<InboxPopulateUpdateState>();

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
pub async fn sent_email_populated(app: AppHandle) -> Result<(), Error> {
    let sent_progress_state = app.state::<SentPopulateUpdateState>();

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
