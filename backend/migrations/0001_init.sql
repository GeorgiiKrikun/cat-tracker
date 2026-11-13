-- Users who can log in. Accounts are created via the CLI (`cattracker add-user`).
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name  TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Login sessions. Only a SHA-256 hash of the cookie token is stored.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_user ON sessions(user_id);

-- Symptom (and medication) definitions. These are data, edited in the app's
-- settings screen, so new symptoms never need a code change.
--
-- kind:
--   bool    one tap = "it happened"                  (value: "yes")
--   count   one tap = one occurrence                 (value: "1")
--   scale   ordered options, e.g. none/little/normal (value: option text)
--   choice  unordered options, e.g. stool type       (value: option text)
--   number  measured value with unit, e.g. weight    (value: "4.2")
--   med     medication dose given                    (value: dose text)
--   note    free text only                           (value: NULL, text in note)
CREATE TABLE symptom_types (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('bool','count','scale','choice','number','med','note')),
    options    TEXT,              -- JSON array of strings for scale/choice
    unit       TEXT,              -- unit for number, default dose for med
    icon       TEXT,              -- emoji shown on the button
    pinned     INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Logged observations. The id is a client-generated UUID so that entries
-- queued while offline can be re-sent safely (inserts are idempotent).
CREATE TABLE entries (
    id          TEXT PRIMARY KEY,
    symptom_id  INTEGER NOT NULL REFERENCES symptom_types(id),
    value       TEXT,
    note        TEXT,
    logged_by   INTEGER REFERENCES users(id) ON DELETE SET NULL,
    occurred_at TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_entries_time ON entries(occurred_at);
CREATE INDEX idx_entries_symptom_time ON entries(symptom_id, occurred_at);

-- Starter set. Everything here can be renamed, archived or reordered in the app.
INSERT INTO symptom_types (name, kind, options, unit, icon, pinned, sort_order) VALUES
    ('Omeprazole',    'med',    NULL, NULL, '💊', 1, 10),
    ('Emeprid',       'med',    NULL, NULL, '💊', 1, 11),
    ('Pronefra',      'med',    NULL, NULL, '💊', 1, 12),
    ('Prednisolone',  'med',    NULL, NULL, '💊', 1, 13),
    ('Appetite',      'scale',  '["none","little","normal","good"]', NULL, '🍽️', 1, 20),
    ('Vomited',       'count',  NULL, NULL, '🤮', 1, 21),
    ('Stool',         'choice', '["normal","soft","diarrhea","hard","with blood"]', NULL, '💩', 1, 22),
    ('Drinking',      'scale',  '["less","normal","more","a lot"]', NULL, '💧', 1, 23),
    ('Peeing',        'scale',  '["less","normal","more","a lot"]', NULL, '🚽', 0, 24),
    ('Night waking',  'scale',  '["mild","moderate","intense"]', NULL, '🌙', 1, 25),
    ('Energy',        'scale',  '["very low","low","normal","playful"]', NULL, '⚡', 0, 26),
    ('Weight',        'number', NULL, 'kg', '⚖️', 0, 30),
    ('Blood glucose', 'number', NULL, 'mmol/L', '🩸', 0, 31),
    ('Note',          'note',   NULL, NULL, '📝', 1, 40);
