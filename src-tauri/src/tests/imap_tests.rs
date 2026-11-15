use crate::auth::AppleKeychainManager;
use crate::db::{MailBox, Provider};
use crate::imap::{FetchedAttachment, ImapCommand, RefreshSummary};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[test]
fn imap_command_fetch_emails_variant_preserves_values() {
    let cancel_token = CancellationToken::new();
    let cmd = ImapCommand::FetchEmails(Provider::Gmail, cancel_token);

    match cmd {
        ImapCommand::FetchEmails(provider, _cancel_token) => {
            assert_eq!(provider.as_ref(), "gmail");
        }
        _ => panic!("expected fetch emails variant"),
    }
}

#[test]
fn imap_command_refresh_emails_variant_preserves_values() {
    let (tx, _rx) = oneshot::channel::<Result<RefreshSummary, String>>();
    let cmd = ImapCommand::RefreshEmails {
        mail_box: MailBox::Sent,
        provider: Provider::Yahoo,
        response_channel: tx,
        username: "Hello".to_string(),
        keychain_manager: AppleKeychainManager::new("test"),
    };

    match cmd {
        ImapCommand::RefreshEmails {
            mail_box, provider, ..
        } => {
            assert_eq!(mail_box.as_ref(), "Sent");
            assert_eq!(provider.as_ref(), "yahoo");
        }
        _ => panic!("expected refresh emails variant"),
    }
}

#[test]
fn imap_command_fetch_attachment_variant_preserves_values() {
    let (tx, _rx) = oneshot::channel::<Result<FetchedAttachment, String>>();
    let cmd = ImapCommand::FetchAttachment {
        mail_box: MailBox::Inbox,
        uid: 42,
        part_id: "2.1".to_string(),
        response_channel: tx,
        username: "user@example.com".to_string(),
        keychain_manager: AppleKeychainManager::new("test"),
        cancel_token: tokio_util::sync::CancellationToken::new(),
    };

    match cmd {
        ImapCommand::FetchAttachment {
            mail_box,
            uid,
            part_id,
            ..
        } => {
            assert_eq!(mail_box, MailBox::Inbox);
            assert_eq!(uid, 42);
            assert_eq!(part_id, "2.1");
        }
        _ => panic!("expected fetch attachment variant"),
    }
}

#[test]
fn imap_command_set_email_read_variant_preserves_values() {
    let (tx, _rx) = oneshot::channel::<Result<(), String>>();
    let cmd = ImapCommand::SetEmailRead {
        mail_box: MailBox::Inbox,
        provider: Provider::Gmail,
        uid: 42,
        read: true,
        response_channel: tx,
        username: "user@example.com".to_string(),
        keychain_manager: AppleKeychainManager::new("test"),
    };

    match cmd {
        ImapCommand::SetEmailRead {
            mail_box,
            provider,
            uid,
            read,
            username,
            ..
        } => {
            assert_eq!(mail_box.as_ref(), "INBOX");
            assert_eq!(provider.as_ref(), "gmail");
            assert_eq!(uid, 42);
            assert!(read);
            assert_eq!(username, "user@example.com");
        }
        _ => panic!("expected set email read variant"),
    }
}

#[test]
fn imap_command_logout_variant_matches() {
    let cmd = ImapCommand::Logout;
    assert!(matches!(cmd, ImapCommand::Logout));
}
