-- Disappearing messages.
--
-- Two tables, because a timer and a deadline are two different facts. The timer
-- is the conversation's setting -- shared with everybody in it, changed by an
-- `EXPIRATION_TIMER_UPDATE` from either end -- and it is cached here because
-- Signal keeps the one-to-one timer nowhere a client can read it back: it
-- arrives in a message and is thereafter the client's to remember. A group's
-- own record carries one and seeds this.
--
-- The deadline is per message and is an *instant*, not a duration, for the same
-- reason a mute is: "gone in an hour" has to mean the same thing after a
-- restart. When the clock starts is Signal's rule and not ours -- the moment a
-- message is sent, for our own, and the moment it is read, for everybody
-- else's -- so the row is written at that moment rather than when the message
-- arrives.
CREATE TABLE IF NOT EXISTS petunia_expire_timers (
    thread   BLOB    PRIMARY KEY,
    is_group BOOLEAN NOT NULL,
    seconds  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS petunia_expiring (
    thread     BLOB    NOT NULL,
    is_group   BOOLEAN NOT NULL,
    ts         INTEGER NOT NULL,
    sender     BLOB    NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (thread, ts)
);

-- The sweep asks one question -- what is due -- and asks it every half minute
-- for as long as the application is running.
CREATE INDEX IF NOT EXISTS petunia_expiring_due ON petunia_expiring (expires_at);
