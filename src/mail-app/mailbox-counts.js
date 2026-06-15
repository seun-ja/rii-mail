import { createAppSqliteClient } from "../shared/sqlite.js";

const MAILBOX_COLUMN_BY_NAME = Object.freeze({
  INBOX: "INBOX",
  Sent: "Sent",
});

export function createMailboxCountClient({ storage, sql } = {}) {
  const sqliteClient = createAppSqliteClient({ storage, sql });

  async function getMailboxCountDb() {
    return sqliteClient.getDb();
  }

  async function fetchMailboxEmailCount(mailbox, provider) {
    const mailboxColumn = MAILBOX_COLUMN_BY_NAME[mailbox];
    if (!mailboxColumn) {
      return null;
    }

    try {
      const db = await getMailboxCountDb();
      if (!db) {
        return null;
      }

      const rows = await db.select(
        `SELECT ${mailboxColumn} AS count FROM total_emails WHERE provider = $1`,
        [provider],
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
