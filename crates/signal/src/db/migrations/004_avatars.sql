-- Which remote picture the cached avatar bytes came from.
--
-- Signal's profile carries the avatar's CDN path, and that path changes when
-- someone changes their picture. Recording it is what lets a refresh be one
-- small profile fetch instead of re-downloading every avatar on every launch --
-- and without it there is no way to notice a change at all, which is why a
-- picture set a week ago never arrived.
CREATE TABLE IF NOT EXISTS petunia_avatar (
    thread TEXT PRIMARY KEY,
    remote TEXT NOT NULL
);
