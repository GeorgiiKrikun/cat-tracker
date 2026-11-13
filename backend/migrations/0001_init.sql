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
--   scale   ordered options, e.g. none/little/normal (option_id)
--   choice  unordered options, e.g. stool type       (option_id)
--   number  measured value with unit, e.g. weight    (value: "4.2")
--   med     medication dose given                    (value: dose text)
--   note    free text only                           (value: NULL, text in note)
CREATE TABLE symptom_types (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('bool','count','scale','choice','number','med','note')),
    unit       TEXT,              -- unit for number, default dose for med
    icon       TEXT,              -- emoji shown on the button
    pinned     INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Options of scale/choice symptoms. Entries point at an option by id, so
-- renaming the label renames it everywhere. Options that have entries are
-- never deleted, only retired (hidden from logging, still shown in history).
-- sort_order is the scale order for scales (worst/least first).
CREATE TABLE symptom_options (
    id         INTEGER PRIMARY KEY,
    symptom_id INTEGER NOT NULL REFERENCES symptom_types(id),
    label      TEXT NOT NULL,
    retired    INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    UNIQUE (id, symptom_id)       -- target of the composite key in entries
);
CREATE INDEX idx_options_symptom ON symptom_options(symptom_id, sort_order);

-- Logged observations. The id is a client-generated UUID so that entries
-- queued while offline can be re-sent safely (inserts are idempotent).
-- option_id is set for scale/choice entries (value is then NULL); the
-- composite key makes sure the option belongs to the entry's symptom.
CREATE TABLE entries (
    id          TEXT PRIMARY KEY,
    symptom_id  INTEGER NOT NULL REFERENCES symptom_types(id),
    option_id   INTEGER,
    value       TEXT,
    note        TEXT,
    logged_by   INTEGER REFERENCES users(id) ON DELETE SET NULL,
    occurred_at TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (option_id, symptom_id) REFERENCES symptom_options(id, symptom_id)
);
CREATE INDEX idx_entries_time ON entries(occurred_at);
CREATE INDEX idx_entries_symptom_time ON entries(symptom_id, occurred_at);
CREATE INDEX idx_entries_option ON entries(option_id);

-- Starter set. Everything here can be renamed, archived or reordered in the app.
INSERT INTO symptom_types (id, name, kind, unit, icon, pinned, sort_order) VALUES
    (1,  'Omeprazole',    'med',    NULL,     '💊', 1, 10),
    (2,  'Emeprid',       'med',    NULL,     '💊', 1, 11),
    (3,  'Pronefra',      'med',    NULL,     '💊', 1, 12),
    (4,  'Prednisolone',  'med',    NULL,     '💊', 1, 13),
    (5,  'Appetite',      'scale',  NULL,     '🍽️', 1, 20),
    (6,  'Vomited',       'count',  NULL,     '🤮', 1, 21),
    (7,  'Stool',         'choice', NULL,     '💩', 1, 22),
    (8,  'Drinking',      'scale',  NULL,     '💧', 1, 23),
    (9,  'Peeing',        'scale',  NULL,     '🚽', 0, 24),
    (10, 'Night waking',  'scale',  NULL,     '🌙', 1, 25),
    (11, 'Energy',        'scale',  NULL,     '⚡', 0, 26),
    (12, 'Weight',        'number', 'kg',     '⚖️', 0, 30),
    (13, 'Blood glucose', 'number', 'mmol/L', '🩸', 0, 31),
    (14, 'Note',          'note',   NULL,     '📝', 1, 40);

INSERT INTO symptom_options (symptom_id, label, sort_order) VALUES
    (5,  'none', 0), (5,  'little', 1), (5,  'normal', 2), (5,  'good', 3),
    (7,  'normal', 0), (7,  'soft', 1), (7,  'diarrhea', 2), (7,  'hard', 3), (7,  'with blood', 4),
    (8,  'less', 0), (8,  'normal', 1), (8,  'more', 2), (8,  'a lot', 3),
    (9,  'less', 0), (9,  'normal', 1), (9,  'more', 2), (9,  'a lot', 3),
    (10, 'mild', 0), (10, 'moderate', 1), (10, 'intense', 2),
    (11, 'very low', 0), (11, 'low', 1), (11, 'normal', 2), (11, 'playful', 3);
