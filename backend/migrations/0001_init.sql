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

-- Whoever is being observed: a cat, or yourself for a migraine diary.
-- Created via the CLI (`cattracker add-patient`).
CREATE TABLE patients (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    icon       TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Which users can see and log for which patient (`cattracker share`).
-- A patient can be shared by several users, or private to one.
CREATE TABLE patient_access (
    patient_id INTEGER NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (patient_id, user_id)
);
CREATE INDEX idx_access_user ON patient_access(user_id);

-- Symptom (and medication) definitions. These are data, edited in the app's
-- settings screen, so new symptoms never need a code change. Each patient
-- has their own list; entries belong to a patient through their symptom.
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
    patient_id INTEGER NOT NULL REFERENCES patients(id),
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('bool','count','scale','choice','number','med','note')),
    unit       TEXT,              -- unit for number, default dose for med
    icon       TEXT,              -- emoji shown on the button
    pinned     INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_symptoms_patient ON symptom_types(patient_id, sort_order);

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
-- logged_by records who logged it, which matters for shared patients.
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

-- Starter symptom sets are presets in backend/src/patients.rs, applied by
-- `cattracker add-patient --preset ...`.
