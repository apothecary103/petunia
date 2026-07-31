-- What has been fetched into the media cache. The files themselves are
-- authoritative about what is cached; this table records what petunia knows
-- about them, and gives later phases somewhere to keep metadata it computes
-- itself, such as audio duration.
CREATE TABLE IF NOT EXISTS petunia_blob (
    digest       TEXT    PRIMARY KEY,
    content_type TEXT    NOT NULL,
    bytes        INTEGER NOT NULL,
    fetched_at   INTEGER NOT NULL
);
