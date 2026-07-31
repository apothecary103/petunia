-- Pinned, archived, muted, and which folder a conversation is in.
--
-- Local only, and deliberately so. Signal keeps pins and archives in its Storage
-- Service, which libsignal-service exposes read-only and presage neither uses
-- nor exposes; folders it has no concept of at all. So these are this device's
-- opinion about the list, which is honest about what they are and survives a
-- restart, which is what makes them worth having.
--
-- `muted_until` is an instant rather than a duration, because "muted for eight
-- hours" has to mean the same thing after a restart.
CREATE TABLE IF NOT EXISTS petunia_thread_flags (
    thread      BLOB    PRIMARY KEY,
    is_group    BOOLEAN NOT NULL,
    pinned      BOOLEAN NOT NULL DEFAULT 0,
    archived    BOOLEAN NOT NULL DEFAULT 0,
    muted_until INTEGER,
    folder      TEXT
);
