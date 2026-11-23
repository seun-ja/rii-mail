-- Messages written before selective MIME synchronization contain full raw
-- payloads and no reliable server part identifiers. Rebuild that local cache
-- once so BODYSTRUCTURE metadata and on-demand attachment downloads are used.
DELETE FROM messages;

UPDATE mailboxes
SET highest_uid_seen = NULL,
    last_synced_at = NULL,
    updated_at = unixepoch();
