PRAGMA foreign_keys = ON;

-- RiiMail had no released users when this schema was introduced. Remove the
-- provider-specific cache tables so every installation starts from the same
-- normalized schema.
DROP TABLE IF EXISTS yahoo_INBOX;
DROP TABLE IF EXISTS yahoo_Sent;
DROP TABLE IF EXISTS yahoo_Spam;
DROP TABLE IF EXISTS gmail_INBOX;
DROP TABLE IF EXISTS gmail_Sent;
DROP TABLE IF EXISTS gmail_Spam;
DROP TABLE IF EXISTS outlook_INBOX;
DROP TABLE IF EXISTS outlook_Sent;
DROP TABLE IF EXISTS outlook_Spam;
DROP TABLE IF EXISTS custom_INBOX;
DROP TABLE IF EXISTS custom_Sent;
DROP TABLE IF EXISTS custom_Spam;
DROP TABLE IF EXISTS total_emails;
DROP TABLE IF EXISTS mailbox_sync_state;

CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    provider TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(provider, email)
);

CREATE TABLE mailboxes (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    server_name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (
        role IN ('inbox', 'sent', 'drafts', 'trash', 'spam', 'archive', 'custom')
    ),
    delimiter TEXT,
    uid_validity INTEGER,
    uid_next INTEGER,
    highest_modseq INTEGER,
    server_message_count INTEGER NOT NULL DEFAULT 0 CHECK (server_message_count >= 0),
    server_unseen_count INTEGER NOT NULL DEFAULT 0 CHECK (server_unseen_count >= 0),
    highest_uid_seen INTEGER,
    last_synced_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    UNIQUE(account_id, server_name)
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    mailbox_id INTEGER NOT NULL,
    uid INTEGER NOT NULL CHECK (uid >= 0),
    rfc_message_id TEXT,
    thread_id TEXT,
    internal_date INTEGER,
    sent_date INTEGER,
    subject TEXT,
    preview TEXT,
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    is_seen INTEGER NOT NULL DEFAULT 0 CHECK (is_seen IN (0, 1)),
    is_flagged INTEGER NOT NULL DEFAULT 0 CHECK (is_flagged IN (0, 1)),
    is_answered INTEGER NOT NULL DEFAULT 0 CHECK (is_answered IN (0, 1)),
    is_draft INTEGER NOT NULL DEFAULT 0 CHECK (is_draft IN (0, 1)),
    is_deleted INTEGER NOT NULL DEFAULT 0 CHECK (is_deleted IN (0, 1)),
    structure_state TEXT NOT NULL DEFAULT 'not_fetched' CHECK (
        structure_state IN ('not_fetched', 'fetching', 'fetched', 'failed')
    ),
    body_state TEXT NOT NULL DEFAULT 'not_fetched' CHECK (
        body_state IN ('not_fetched', 'fetching', 'fetched', 'failed')
    ),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id) ON DELETE CASCADE,
    UNIQUE(mailbox_id, uid)
);

CREATE TABLE message_addresses (
    id INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('from', 'sender', 'reply_to', 'to', 'cc', 'bcc')),
    position INTEGER NOT NULL DEFAULT 0 CHECK (position >= 0),
    name TEXT,
    address TEXT NOT NULL,
    FOREIGN KEY(message_id) REFERENCES messages(id) ON DELETE CASCADE,
    UNIQUE(message_id, kind, position)
);

CREATE TABLE labels (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    UNIQUE(account_id, name)
);

CREATE TABLE message_labels (
    message_id INTEGER NOT NULL,
    label_id INTEGER NOT NULL,
    PRIMARY KEY(message_id, label_id),
    FOREIGN KEY(message_id) REFERENCES messages(id) ON DELETE CASCADE,
    FOREIGN KEY(label_id) REFERENCES labels(id) ON DELETE CASCADE
);

CREATE TABLE message_content (
    message_id INTEGER PRIMARY KEY,
    text_body TEXT,
    html_body TEXT,
    fetched_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY(message_id) REFERENCES messages(id) ON DELETE CASCADE
);

CREATE TABLE mime_parts (
    id INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL,
    section TEXT NOT NULL,
    parent_section TEXT,
    content_type TEXT NOT NULL,
    charset TEXT,
    disposition TEXT CHECK (
        disposition IS NULL OR disposition IN ('inline', 'attachment')
    ),
    filename TEXT,
    content_id TEXT,
    transfer_encoding TEXT,
    encoded_size INTEGER CHECK (encoded_size IS NULL OR encoded_size >= 0),
    decoded_size INTEGER CHECK (decoded_size IS NULL OR decoded_size >= 0),
    is_body_candidate INTEGER NOT NULL DEFAULT 0 CHECK (is_body_candidate IN (0, 1)),
    FOREIGN KEY(message_id) REFERENCES messages(id) ON DELETE CASCADE,
    UNIQUE(message_id, section)
);

CREATE TABLE part_cache (
    part_id INTEGER PRIMARY KEY,
    relative_path TEXT NOT NULL UNIQUE,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    checksum TEXT,
    state TEXT NOT NULL CHECK (state IN ('downloading', 'ready', 'failed', 'evicting')),
    cached_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_accessed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY(part_id) REFERENCES mime_parts(id) ON DELETE CASCADE
);

CREATE TABLE cache_deletion_queue (
    relative_path TEXT PRIMARY KEY,
    queued_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TRIGGER queue_part_cache_file_before_delete
BEFORE DELETE ON part_cache
BEGIN
    INSERT OR IGNORE INTO cache_deletion_queue (relative_path)
    VALUES (OLD.relative_path);
END;

CREATE TABLE drafts (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    mailbox_id INTEGER,
    server_uid INTEGER,
    subject TEXT,
    text_body TEXT,
    html_body TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id) ON DELETE SET NULL
);

CREATE TABLE draft_addresses (
    id INTEGER PRIMARY KEY,
    draft_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('to', 'cc', 'bcc', 'reply_to')),
    position INTEGER NOT NULL DEFAULT 0 CHECK (position >= 0),
    name TEXT,
    address TEXT NOT NULL,
    FOREIGN KEY(draft_id) REFERENCES drafts(id) ON DELETE CASCADE,
    UNIQUE(draft_id, kind, position)
);

CREATE TABLE draft_attachments (
    id INTEGER PRIMARY KEY,
    draft_id INTEGER NOT NULL,
    relative_path TEXT NOT NULL,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    checksum TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY(draft_id) REFERENCES drafts(id) ON DELETE CASCADE
);

CREATE INDEX messages_by_mailbox_date
    ON messages(mailbox_id, internal_date DESC, uid DESC);
CREATE INDEX unread_messages_by_mailbox_date
    ON messages(mailbox_id, internal_date DESC) WHERE is_seen = 0;
CREATE INDEX message_addresses_by_address
    ON message_addresses(address);
CREATE INDEX message_addresses_by_message
    ON message_addresses(message_id, kind, position);
CREATE INDEX message_labels_by_label
    ON message_labels(label_id, message_id);
CREATE INDEX mime_parts_by_message
    ON mime_parts(message_id, section);
CREATE INDEX cached_parts_by_access
    ON part_cache(last_accessed_at) WHERE state = 'ready';

