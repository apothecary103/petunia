-- How far each conversation has been read. Unread counts lived only in memory
-- and so reset on every restart; and reading a message on the phone tells this
-- client nothing unless what it hears is written down.
--
-- One row per thread rather than one per message: Signal's own read state is a
-- watermark, and a message older than the mark is read whether or not a receipt
-- for it was ever seen here.
CREATE TABLE IF NOT EXISTS petunia_read (
    thread    TEXT    PRIMARY KEY,
    read_upto INTEGER NOT NULL
);
