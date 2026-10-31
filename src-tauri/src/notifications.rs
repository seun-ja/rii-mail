use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
};

use arc_swap::ArcSwap;
#[cfg(target_os = "macos")]
use tauri::Emitter as _;
use tauri::Manager as _;

use crate::{
    config::{AppState, InitializedState},
    db::{get_email_by_uid, MailBox, Provider},
    email_cache::FrontendEmail,
    error::Error,
};

#[derive(Default)]
pub(crate) struct NotificationClicks {
    queue: Mutex<VecDeque<NotificationTarget>>,
    #[cfg(target_os = "macos")]
    identity: Mutex<Option<macos::NotificationIdentity>>,
}

struct NotificationTarget {
    uid: u32,
    provider: Provider,
    // A notification must never open the same UID in a different login session.
    session: Weak<InitializedState>,
}

pub(crate) fn current_session(
    app: &tauri::AppHandle,
    provider: &Provider,
) -> Option<Weak<InitializedState>> {
    let state = app.state::<ArcSwap<AppState>>();
    let current = state.load();
    match current.as_ref() {
        AppState::Initialized(session)
            if session
                .accounts
                .first()
                .is_some_and(|account| &account.provider == provider) =>
        {
            Some(Arc::downgrade(session))
        }
        _ => None,
    }
}

fn is_current_session(app: &tauri::AppHandle, session: &Arc<InitializedState>) -> bool {
    let state = app.state::<ArcSwap<AppState>>();
    matches!(state.load().as_ref(), AppState::Initialized(current) if Arc::ptr_eq(current, session))
}

#[tauri::command]
pub(crate) async fn take_notification_email(
    app: tauri::AppHandle,
) -> Result<Option<FrontendEmail>, Error> {
    loop {
        let target = app
            .state::<NotificationClicks>()
            .queue
            .lock()
            .map_err(|_| Error::Other("Notification click queue is unavailable".into()))?
            .pop_front();
        let Some(target) = target else {
            return Ok(None);
        };
        let Some(session) = target.session.upgrade() else {
            continue;
        };
        if !is_current_session(&app, &session) {
            continue;
        }
        let table = format!("{}_{}", target.provider.as_ref(), MailBox::Inbox.as_ref());
        let email = get_email_by_uid(&session.sqlite_pool, &table, target.uid).await?;
        if !is_current_session(&app, &session) {
            return Ok(None);
        }
        let Some(email) = email else {
            tracing::warn!(
                uid = target.uid,
                "Notification's message is no longer in the local inbox"
            );
            continue;
        };
        let mut frontend = email.into_frontend(format!("db-INBOX-uid-{}", target.uid));
        frontend.folder = "INBOX".into();
        return Ok(Some(frontend));
    }
}

#[cfg(target_os = "macos")]
fn notification_clicked(app: tauri::AppHandle, target: NotificationTarget) {
    let Some(session) = target.session.upgrade() else {
        return;
    };
    if !is_current_session(&app, &session) {
        return;
    }
    let clicks = app.state::<NotificationClicks>();
    match clicks.queue.lock() {
        Ok(mut queue) => queue.push_back(target),
        Err(error) => {
            tracing::error!(%error, "Failed to queue notification click");
            return;
        }
    }
    if let Some(window) = app
        .get_webview_window("main-app")
        .or_else(|| app.get_webview_window("main"))
    {
        if let Err(error) = window
            .show()
            .and_then(|_| window.unminimize())
            .and_then(|_| window.set_focus())
        {
            tracing::warn!(?error, "Failed to bring notification's window forward");
        }
    }
    // Keep the target queued if the webview is still loading; it also checks on startup.
    if let Err(error) = app.emit("app://notification-clicked", ()) {
        tracing::warn!(?error, "Failed to signal notification click to mail reader");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn show_new_mail(
    app: &tauri::AppHandle,
    session: Weak<InitializedState>,
    provider: Provider,
    uid: u32,
    subject: String,
) {
    let callback_app = app.clone();
    let target = NotificationTarget {
        uid,
        provider,
        session,
    };
    if let Err(error) = app.run_on_main_thread(move || macos::show(callback_app, target, subject)) {
        tracing::warn!(?error, uid, "Failed to schedule mail notification");
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        ffi::{c_char, CStr, CString},
        sync::OnceLock,
    };

    pub(super) struct NotificationIdentity {
        id: String,
        session: Weak<InitializedState>,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct NotificationAddress {
        session: String,
        provider: String,
        uid: u32,
    }

    impl NotificationAddress {
        fn target(&self, identity: &NotificationIdentity) -> Option<NotificationTarget> {
            if self.session != identity.id {
                return None;
            }
            let provider = match self.provider.as_str() {
                "gmail" => Provider::Gmail,
                "yahoo" => Provider::Yahoo,
                "outlook" => Provider::Outlook,
                "custom" => Provider::CustomImapServer,
                _ => return None,
            };
            Some(NotificationTarget {
                uid: self.uid,
                provider,
                session: identity.session.clone(),
            })
        }
    }

    static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
    static APPLICATION: OnceLock<Result<(), String>> = OnceLock::new();

    extern "C" {
        fn riimail_show_notification(
            identifier: *const c_char,
            subject: *const c_char,
            callback: extern "C" fn(*const c_char, bool),
        );
    }

    extern "C" fn response(identifier: *const c_char, clicked: bool) {
        let Some(app) = APP.get() else { return };
        if !clicked || identifier.is_null() {
            return;
        }
        // The delegate passes an NSString's UTF-8 buffer, valid for this call.
        let Ok(identifier) = (unsafe { CStr::from_ptr(identifier) }).to_str() else {
            return;
        };
        let Ok(address) = serde_json::from_str::<NotificationAddress>(identifier) else {
            return;
        };
        let target = match app.state::<NotificationClicks>().identity.lock() {
            Ok(identity) => identity
                .as_ref()
                .and_then(|identity| address.target(identity)),
            Err(error) => {
                tracing::error!(%error, "Failed to resolve notification response");
                return;
            }
        };
        if let Some(target) = target {
            let callback_app = app.clone();
            if let Err(error) =
                app.run_on_main_thread(move || notification_clicked(callback_app, target))
            {
                tracing::warn!(?error, "Failed to handle notification click");
            }
        }
    }

    pub(super) fn show(app: tauri::AppHandle, target: NotificationTarget, subject: String) {
        let Some(session) = target.session.upgrade() else {
            return;
        };
        if !is_current_session(&app, &session) {
            return;
        }
        let identity = APPLICATION.get_or_init(|| {
            mac_notification_sys::set_application(if tauri::is_dev() {
                "com.apple.Terminal"
            } else {
                &app.config().identifier
            })
            .map_err(|error| error.to_string())
        });
        if let Err(error) = identity {
            tracing::warn!(%error, "Failed to configure macOS notification identity");
            return;
        }
        let _ = APP.set(app.clone());
        let session_id = match app.state::<NotificationClicks>().identity.lock() {
            Ok(mut identity) => {
                if !identity
                    .as_ref()
                    .is_some_and(|identity| identity.session.ptr_eq(&target.session))
                {
                    *identity = Some(NotificationIdentity {
                        id: uuid::Uuid::new_v4().to_string(),
                        session: target.session,
                    });
                }
                identity
                    .as_ref()
                    .expect("Notification identity was initialized")
                    .id
                    .clone()
            }
            Err(error) => {
                tracing::error!(%error, "Failed to register mail notification");
                return;
            }
        };
        // Store the address in the native notification, not a per-message Rust
        // map: unclicked or suppressed banners must not accumulate allocations.
        let address = NotificationAddress {
            session: session_id,
            provider: target.provider.as_ref().to_string(),
            uid: target.uid,
        };
        let identifier =
            serde_json::to_string(&address).expect("Notification address is serializable");
        let native_identifier = CString::new(identifier).expect("JSON escapes NUL bytes");
        let subject = CString::new(subject.replace('\0', "�")).expect("NUL bytes were replaced");
        // Runs on the main thread. Native code copies both strings before return,
        // and invokes the static callback later; no Rust pointers are retained.
        unsafe {
            riimail_show_notification(native_identifier.as_ptr(), subject.as_ptr(), response)
        };
        tracing::info!("Notification request submitted; OS delivery is asynchronous");
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn identity() -> NotificationIdentity {
            NotificationIdentity {
                id: "current-login".into(),
                session: Weak::new(),
            }
        }

        #[test]
        fn notification_address_roundtrips_exact_provider_and_uid() {
            let address = NotificationAddress {
                session: "current-login".into(),
                provider: "custom".into(),
                uid: u32::MAX,
            };
            let encoded = serde_json::to_string(&address).unwrap();
            let decoded: NotificationAddress = serde_json::from_str(&encoded).unwrap();
            let target = decoded.target(&identity()).unwrap();
            assert_eq!(target.uid, u32::MAX);
            assert_eq!(target.provider, Provider::CustomImapServer);
        }

        #[test]
        fn notifications_from_previous_login_cannot_resolve() {
            let address = NotificationAddress {
                session: "previous-login".into(),
                provider: "gmail".into(),
                uid: 42,
            };
            assert!(address.target(&identity()).is_none());
        }

        #[test]
        fn malformed_notification_addresses_cannot_resolve() {
            assert!(serde_json::from_str::<NotificationAddress>("not an address").is_err());
            let address = NotificationAddress {
                session: "current-login".into(),
                provider: "unknown".into(),
                uid: 42,
            };
            assert!(address.target(&identity()).is_none());
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_new_mail(
    app: &tauri::AppHandle,
    _session: Weak<InitializedState>,
    _provider: Provider,
    _uid: u32,
    subject: String,
) {
    use tauri_plugin_notification::NotificationExt as _;
    if let Err(error) = app
        .notification()
        .builder()
        .title("New Mail")
        .body(subject)
        .show()
    {
        tracing::warn!(?error, "Failed to send mail notification");
    }
}
