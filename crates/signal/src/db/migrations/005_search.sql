-- What messages said, so they can be found again.
--
-- presage stores an encoded protobuf and nothing else, so there is no column to
-- match a query against. This is filled as threads are read, which is when the
-- rows have already been decoded and the bodies are in hand anyway.
--
-- `is_group` is here because a thread key is either a uuid or a group master
-- key and the two are both opaque blobs; without it a row cannot say which it
-- came from.
CREATE TABLE IF NOT EXISTS petunia_body (
    ts       INTEGER NOT NULL,
    thread   BLOB    NOT NULL,
    is_group BOOLEAN NOT NULL,
    sender   BLOB    NOT NULL,
    body     TEXT    NOT NULL,
    PRIMARY KEY (ts, thread)
);

-- Results come back newest first, and a scoped search filters by thread before
-- it orders.
CREATE INDEX IF NOT EXISTS petunia_body_thread_ts ON petunia_body(thread, ts DESC);
