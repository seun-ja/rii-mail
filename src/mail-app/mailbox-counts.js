import { createAppSqliteClient } from "../shared/sqlite.js";

const SERVER_MAILBOX_BY_NAME = Object.freeze({
  INBOX: "INBOX",
  Sent: "Sent",
});

export function createMailboxCountClient({ storage, sql } = {}) {
  const sqliteClient = createAppSqliteClient({ storage, sql });

  async function getMailboxCountDb() {
    return sqliteClient.getDb();
  }

  async function fetchMailboxEmailCount(mailbox, provider) {
    const serverMailbox = SERVER_MAILBOX_BY_NAME[mailbox];
    if (!serverMailbox) {
      return null;
    }

    try {
      const db = await getMailboxCountDb();
      if (!db) {
        return null;
      }

      const rows = await db.select(
        `SELECT m.server_message_count AS count
         FROM mailboxes m
         JOIN accounts a ON a.id = m.account_id
         WHERE a.provider = $1 AND m.server_name = $2
         ORDER BY a.id
         LIMIT 1`,
        [provider, serverMailbox],
      );

      const count = Array.isArray(rows) ? rows[0]?.count : null;
      return typeof count === "number" ? count : null;
    } catch (error) {
      console.warn("Failed to fetch mailbox email count", error);
      return null;
    }
  }

  return {
    getMailboxCountDb,
    fetchMailboxEmailCount,
  };
}
