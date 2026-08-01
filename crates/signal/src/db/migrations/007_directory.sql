-- What the sidebar shows and what people are called, kept rather than derived.
--
-- Both used to be rebuilt from scratch at every launch, from sources that can
-- come back empty. A preview came from scanning a thread's newest stored rows
-- and projecting them; a page whose rows are all reactions, edits and tombstones
-- projects to nothing, so no preview was produced -- and a conversation with no
-- preview was not listed at all. The whole history sat on disk and the person was
-- gone from the list. A name came from a profile fetch per contact, run serially
-- over the address book, after deleting the cache the previous run had filled: a
-- launch that ended before the crawl did left the rest of the list as uuid
-- fragments, and the next one started over from nothing.
--
-- So both are written the moment they are learned. The network then makes them
-- *current*, which is a job it can fail at without anybody vanishing.
CREATE TABLE IF NOT EXISTS petunia_preview (
    thread   BLOB    PRIMARY KEY,
    is_group BOOLEAN NOT NULL,
    at       INTEGER NOT NULL,
    line     TEXT    NOT NULL
);

-- One row per person rather than one per source. A name has three of them and
-- which one wins is the view's decision, so all three are kept side by side and
-- none overwrites another.
CREATE TABLE IF NOT EXISTS petunia_name (
    uuid     BLOB PRIMARY KEY,
    profile  TEXT,
    nickname TEXT,
    note     TEXT
);
