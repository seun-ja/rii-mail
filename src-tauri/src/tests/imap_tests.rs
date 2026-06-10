use crate::db::{MailBox, Providers};
use crate::imap::{ImapCommand, RefreshSummary};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[test]
fn imap_command_fetch_emails_variant_preserves_values() {
    let cancel_token = CancellationToken::new();
    let cmd = ImapCommand::FetchEmails(MailBox::Inbox, Providers::Gmail, cancel_token);

    match cmd {
        ImapCommand::FetchEmails(mailbox, provider, _cancel_token) => {
            assert_eq!(mailbox.as_ref(), "INBOX");
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
        provider: Providers::Yahoo,
        response_channel: tx,
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
fn imap_command_logout_variant_matches() {
    let cmd = ImapCommand::Logout;
    assert!(matches!(cmd, ImapCommand::Logout));
}
