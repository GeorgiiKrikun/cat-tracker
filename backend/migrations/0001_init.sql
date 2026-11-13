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

-- Trackers: everything that can be logged for a patient — medications,
-- events (vomited, a migraine attack), observations (appetite, stool) and
-- measurements (weight). These are data, edited in the app's settings
-- screen, so new trackers never need a code change. Each patient has their
-- own list; entries belong to a patient through their tracker.
--
-- What a day without an entry means depends on the kind: nothing happened
-- (bool, count, med), unknown (number), or the tracker's normal option
-- (scale/choice that have one), but only on days someone logged anything
-- for the patient (an entry or a check-in); otherwise nobody looked.
--
-- kind:
--   bool    one tap = "it happened"                  (value: "yes")
--   count   one tap = one occurrence                 (value: "1")
--   scale   ordered options, e.g. none/little/normal (option_id)
--   choice  unordered options, e.g. stool type       (option_id)
--   number  measured value with unit, e.g. weight    (value: "4.2")
--   med     medication dose given                    (value: dose text)
--   note    free text only                           (value: NULL, text in note)
CREATE TABLE trackers (
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
CREATE INDEX idx_trackers_patient ON trackers(patient_id, sort_order);

-- Options of scale/choice trackers. Entries point at an option by id, so
-- renaming the label renames it everywhere. Options that have entries are
-- never deleted, only retired (hidden from logging, still shown in history).
-- sort_order is the scale order for scales (worst/least first).
-- At most one active option per tracker is `normal`: the usual state,
-- assumed on observed days where nothing was logged for the tracker.
CREATE TABLE tracker_options (
    id         INTEGER PRIMARY KEY,
    tracker_id INTEGER NOT NULL REFERENCES trackers(id),
    label      TEXT NOT NULL,
    retired    INTEGER NOT NULL DEFAULT 0,
    normal     INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    UNIQUE (id, tracker_id),      -- target of the composite key in entries
    CHECK (NOT (normal AND retired))
);
CREATE INDEX idx_options_tracker ON tracker_options(tracker_id, sort_order);
CREATE UNIQUE INDEX idx_options_one_normal ON tracker_options(tracker_id) WHERE normal;

-- Logged observations. The id is a client-generated UUID so that entries
-- queued while offline can be re-sent safely (inserts are idempotent).
-- logged_by records who logged it, which matters for shared patients.
-- option_id is set for scale/choice entries (value is then NULL); the
-- composite key makes sure the option belongs to the entry's tracker.
CREATE TABLE entries (
    id          TEXT PRIMARY KEY,
    tracker_id  INTEGER NOT NULL REFERENCES trackers(id),
    option_id   INTEGER,
    value       TEXT,
    note        TEXT,
    logged_by   INTEGER REFERENCES users(id) ON DELETE SET NULL,
    occurred_at TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (option_id, tracker_id) REFERENCES tracker_options(id, tracker_id)
);
CREATE INDEX idx_entries_time ON entries(occurred_at);
CREATE INDEX idx_entries_tracker_time ON entries(tracker_id, occurred_at);
CREATE INDEX idx_entries_option ON entries(option_id);

-- "Nothing unusual today": marks a day as observed when there was nothing
-- else to log, so normal options are assumed for it. Client-generated UUID
-- like entries, for the offline queue.
CREATE TABLE checkins (
    id          TEXT PRIMARY KEY,
    patient_id  INTEGER NOT NULL REFERENCES patients(id),
    logged_by   INTEGER REFERENCES users(id) ON DELETE SET NULL,
    occurred_at TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_checkins_patient_time ON checkins(patient_id, occurred_at);

-- Starter tracker sets are presets in backend/src/patients.rs, applied by
-- `cattracker add-patient --preset ...`.
