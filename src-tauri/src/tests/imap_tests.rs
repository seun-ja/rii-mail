use crate::db::{MailBox, Providers};
use crate::imap::ImapCommand;
use tokio::sync::oneshot;

#[test]
fn imap_command_fetch_emails_variant_preserves_values() {
    let cmd = ImapCommand::FetchEmails(MailBox::Inbox, Providers::Gmail);

    match cmd {
        ImapCommand::FetchEmails(mailbox, provider) => {
            assert_eq!(mailbox.as_ref(), "INBOX");
            assert_eq!(provider.as_ref(), "gmail");
        }
        _ => panic!("expected fetch emails variant"),
    }
}

#[test]
fn imap_command_refresh_emails_variant_preserves_values() {
    let (tx, _rx) = oneshot::channel::<Result<u16, String>>();
    let cmd = ImapCommand::RefreshEmails(MailBox::Sent, Providers::Yahoo, tx);

    match cmd {
        ImapCommand::RefreshEmails(mailbox, provider, _sender) => {
            assert_eq!(mailbox.as_ref(), "Sent");
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
