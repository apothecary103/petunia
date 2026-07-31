-- Local send lifecycle for our own messages: Sending, Failed, Sent.
CREATE TABLE IF NOT EXISTS petunia_send (
    ts     INTEGER PRIMARY KEY,
    thread BLOB    NOT NULL,
    state  INTEGER NOT NULL
);

-- A ReceiptMessage names timestamps but not a recipient, so the sender is taken
-- from the envelope metadata. In a group every member sends their own receipt,
-- so keying on the timestamp alone would report "read" once one member reads.
CREATE TABLE IF NOT EXISTS petunia_receipt (
    ts        INTEGER NOT NULL,
    recipient BLOB    NOT NULL,
    state     INTEGER NOT NULL,
    PRIMARY KEY (ts, recipient)
);

CREATE INDEX IF NOT EXISTS petunia_receipt_ts ON petunia_receipt (ts);

-- presage's primary key is (ts, thread_id), so a per-thread newest-first scan
-- cannot seek and reverse-scans the global index instead.
CREATE INDEX IF NOT EXISTS petunia_thread_messages_thread_ts
    ON thread_messages (thread_id, ts DESC);

-- Created first so the back-fill works on both a fresh database and one that
-- still carries the timestamp-only table this replaces.
CREATE TABLE IF NOT EXISTS petunia_message_status (
    timestamp INTEGER PRIMARY KEY,
    status    INTEGER NOT NULL
);

INSERT OR IGNORE INTO petunia_send (ts, thread, state)
    SELECT timestamp, X'', status FROM petunia_message_status WHERE status <= 2;

-- The old table never recorded who receipted, so the back-filled rows use a nil
-- recipient meaning "at least one, unattributed".
INSERT OR IGNORE INTO petunia_receipt (ts, recipient, state)
    SELECT timestamp, X'00000000000000000000000000000000', status
    FROM petunia_message_status WHERE status >= 3;

DROP TABLE petunia_message_status;
