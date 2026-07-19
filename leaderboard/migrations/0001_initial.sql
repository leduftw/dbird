CREATE TABLE players (
    username_key TEXT PRIMARY KEY,
    display_username TEXT NOT NULL,
    credential_hash TEXT NOT NULL,
    high_score INTEGER NOT NULL DEFAULT 0 CHECK (high_score BETWEEN 0 AND 4294967295),
    created_at INTEGER NOT NULL,
    achieved_at INTEGER
) WITHOUT ROWID;

CREATE INDEX players_leaderboard
    ON players (high_score DESC, achieved_at ASC, username_key ASC);
