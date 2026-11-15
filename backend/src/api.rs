//! JSON API under /api.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    AppState,
    auth::{self, CurrentUser},
    error::{AppError, AppResult},
    normalize_ts, now_ts,
    patients::{self, Patient},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/version", get(version))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/patients", get(patients::list))
        // Everything below belongs to one patient; the `Patient` extractor checks access.
        .route("/patients/{pid}/trackers", get(list_trackers).post(create_tracker))
        .route("/patients/{pid}/trackers/order", put(reorder_trackers))
        .route("/patients/{pid}/trackers/{id}", patch(update_tracker))
        .route("/patients/{pid}/trackers/{id}/options/{option_id}/merge", post(merge_option))
        .route("/patients/{pid}/entries", get(list_entries).post(create_entry))
        .route("/patients/{pid}/entries/{id}", patch(update_entry).delete(delete_entry))
        .route("/patients/{pid}/checkins", get(list_checkins).post(create_checkin))
        .route("/patients/{pid}/checkins/{id}", delete(delete_checkin))
}

/// Which build is running; `sha` is the commit (set by the Nix build), "dev" otherwise.
async fn version() -> Json<serde_json::Value> {
    let sha = std::env::var("CT_GIT_SHA").unwrap_or_else(|_| "dev".into());
    Json(json!({ "version": env!("CARGO_PKG_VERSION"), "sha": sha }))
}

// ---------- auth ----------

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

async fn login(
    State(st): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginReq>,
) -> AppResult<impl IntoResponse> {
    let row: Option<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, username, display_name, password_hash FROM users WHERE username = ?",
    )
    .bind(req.username.trim())
    .fetch_optional(&st.db)
    .await?;

    let password = req.password.clone();
    let ok_user = tokio::task::spawn_blocking(move || match row {
        Some((id, username, display_name, hash)) if auth::verify_password(&password, &hash) => {
            Some((id, username, display_name))
        }
        Some(_) => None,
        None => {
            auth::dummy_verify(&password);
            None
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;

    let Some((id, username, display_name)) = ok_user else {
        // Slow down guessing a little on top of argon2's cost.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        return Err(AppError::BadRequest("wrong username or password".into()));
    };

    let token = auth::create_session(&st.db, id).await?;
    let jar = jar.add(auth::session_cookie(token, st.cookie_secure));
    Ok((
        jar,
        Json(json!({ "id": id, "username": username, "display_name": display_name })),
    ))
}

async fn logout(State(st): State<AppState>, jar: CookieJar) -> AppResult<impl IntoResponse> {
    if let Some(c) = jar.get(auth::SESSION_COOKIE) {
        auth::delete_session(&st.db, c.value()).await?;
    }
    Ok((
        jar.add(auth::removal_cookie(st.cookie_secure)),
        StatusCode::NO_CONTENT,
    ))
}

async fn me(user: CurrentUser) -> Json<CurrentUser> {
    Json(user)
}

// ---------- tracker types ----------

pub const KINDS: &[&str] = &["bool", "count", "scale", "choice", "number", "med", "note"];

fn has_options(kind: &str) -> bool {
    kind == "scale" || kind == "choice"
}

#[derive(sqlx::FromRow)]
struct TrackerRow {
    id: i64,
    name: String,
    kind: String,
    unit: Option<String>,
    icon: Option<String>,
    pinned: bool,
    archived: bool,
    sort_order: i64,
    created_at: String,
}

#[derive(Serialize, sqlx::FromRow, Clone)]
struct TrackerOption {
    id: i64,
    #[serde(skip)]
    tracker_id: i64,
    label: String,
    retired: bool,
    /// The usual state, assumed on observed days without an entry.
    normal: bool,
    /// Number of entries using this option.
    uses: i64,
}

#[derive(Serialize)]
pub struct Tracker {
    id: i64,
    name: String,
    kind: String,
    /// In order (scale order for scales), retired ones included.
    options: Vec<TrackerOption>,
    unit: Option<String>,
    icon: Option<String>,
    pinned: bool,
    archived: bool,
    sort_order: i64,
    /// Normal options aren't assumed for days before this.
    created_at: String,
}

impl Tracker {
    fn new(r: TrackerRow, options: Vec<TrackerOption>) -> Self {
        Tracker {
            id: r.id,
            name: r.name,
            kind: r.kind,
            options,
            unit: r.unit,
            icon: r.icon,
            pinned: r.pinned,
            archived: r.archived,
            sort_order: r.sort_order,
            created_at: r.created_at,
        }
    }
}

/// A tracker of this patient (404 for other patients' trackers).
async fn fetch_tracker(st: &AppState, pid: i64, id: i64) -> AppResult<TrackerRow> {
    sqlx::query_as(
        "SELECT id, name, kind, unit, icon, pinned, archived, sort_order, created_at
           FROM trackers WHERE id = ? AND patient_id = ?",
    )
    .bind(id)
    .bind(pid)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// Options of one tracker, or of all the patient's trackers when `tracker_id` is None.
async fn fetch_options(st: &AppState, pid: i64, tracker_id: Option<i64>) -> AppResult<Vec<TrackerOption>> {
    Ok(sqlx::query_as(
        "SELECT o.id, o.tracker_id, o.label, o.retired, o.normal,
                (SELECT COUNT(*) FROM entries e WHERE e.option_id = o.id) AS uses
           FROM tracker_options o
           JOIN trackers t ON t.id = o.tracker_id
          WHERE t.patient_id = ?1 AND (?2 IS NULL OR o.tracker_id = ?2)
          ORDER BY o.tracker_id, o.sort_order, o.id",
    )
    .bind(pid)
    .bind(tracker_id)
    .fetch_all(&st.db)
    .await?)
}

async fn tracker_with_options(st: &AppState, pid: i64, id: i64) -> AppResult<Tracker> {
    let row = fetch_tracker(st, pid, id).await?;
    Ok(Tracker::new(row, fetch_options(st, pid, Some(id)).await?))
}

#[derive(Deserialize)]
struct ListTrackersQ {
    #[serde(default)]
    include_archived: bool,
}

async fn list_trackers(
    p: Patient,
    State(st): State<AppState>,
    Query(q): Query<ListTrackersQ>,
) -> AppResult<Json<Vec<Tracker>>> {
    let rows: Vec<TrackerRow> = sqlx::query_as(
        "SELECT id, name, kind, unit, icon, pinned, archived, sort_order, created_at
           FROM trackers
          WHERE patient_id = ? AND (? OR archived = 0)
          ORDER BY sort_order, id",
    )
    .bind(p.id)
    .bind(q.include_archived)
    .fetch_all(&st.db)
    .await?;
    let mut by_tracker: HashMap<i64, Vec<TrackerOption>> = HashMap::new();
    for o in fetch_options(&st, p.id, None).await? {
        by_tracker.entry(o.tracker_id).or_default().push(o);
    }
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                let opts = by_tracker.remove(&r.id).unwrap_or_default();
                Tracker::new(r, opts)
            })
            .collect(),
    ))
}

/// One option in the editor's list. `id` is None for a new option.
#[derive(Deserialize)]
struct OptionInput {
    id: Option<i64>,
    label: String,
    #[serde(default)]
    retired: bool,
    #[serde(default)]
    normal: bool,
}

#[derive(Deserialize)]
struct TrackerInput {
    name: Option<String>,
    kind: Option<String>,
    /// The full option list, in order. Existing options left out are deleted,
    /// which is only allowed while no entry uses them.
    options: Option<Vec<OptionInput>>,
    unit: Option<Option<String>>,
    icon: Option<Option<String>>,
    pinned: Option<bool>,
    archived: Option<bool>,
}

fn clean_opt_text(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Check an option list before saving: non-empty unique labels, ids that
/// belong to this tracker, at least two options left to log with, at most
/// one normal option and not a retired one.
fn check_options(inp: &mut [OptionInput], existing: &[TrackerOption]) -> AppResult<()> {
    let bad = |m: String| Err(AppError::BadRequest(m));
    let mut seen = std::collections::HashSet::new();
    for o in inp.iter_mut() {
        o.label = o.label.trim().to_string();
        if o.label.is_empty() {
            return bad("options can't be empty".into());
        }
        if let Some(id) = o.id
            && !existing.iter().any(|e| e.id == id)
        {
            return bad(format!("option {id} doesn't belong to this tracker"));
        }
        if !seen.insert(o.label.to_lowercase()) {
            return bad(format!("“{}” is listed twice", o.label));
        }
        if o.normal && o.retired {
            return bad(format!("“{}” is retired, so it can't be the normal option", o.label));
        }
    }
    if inp.iter().filter(|o| o.normal).count() > 1 {
        return bad("only one option can be the normal one".into());
    }
    if inp.iter().filter(|o| !o.retired).count() < 2 {
        return bad("scale and choice need at least two active options".into());
    }
    Ok(())
}

/// Write an already checked option list for a tracker (inside a transaction).
async fn save_options(
    tx: &mut sqlx::SqliteConnection,
    tracker_id: i64,
    inp: &[OptionInput],
    existing: &[TrackerOption],
) -> AppResult<()> {
    for e in existing {
        if inp.iter().any(|o| o.id == Some(e.id)) {
            continue;
        }
        if e.uses > 0 {
            return Err(AppError::BadRequest(format!(
                "“{}” is used by {} entries; retire it or merge it into another option instead",
                e.label, e.uses
            )));
        }
        sqlx::query("DELETE FROM tracker_options WHERE id = ?")
            .bind(e.id)
            .execute(&mut *tx)
            .await?;
    }
    // Cleared first: the unique index allows one normal option at any moment.
    sqlx::query("UPDATE tracker_options SET normal = 0 WHERE tracker_id = ?")
        .bind(tracker_id)
        .execute(&mut *tx)
        .await?;
    for (i, o) in inp.iter().enumerate() {
        match o.id {
            Some(id) => {
                sqlx::query(
                    "UPDATE tracker_options SET label = ?, retired = ?, normal = ?, sort_order = ? WHERE id = ?",
                )
                .bind(&o.label)
                .bind(o.retired)
                .bind(o.normal)
                .bind(i as i64)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query(
                    "INSERT INTO tracker_options (tracker_id, label, retired, normal, sort_order)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(tracker_id)
                .bind(&o.label)
                .bind(o.retired)
                .bind(o.normal)
                .bind(i as i64)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    Ok(())
}

async fn create_tracker(
    p: Patient,
    State(st): State<AppState>,
    Json(inp): Json<TrackerInput>,
) -> AppResult<(StatusCode, Json<Tracker>)> {
    let name = clean_opt_text(inp.name).ok_or(AppError::BadRequest("name is required".into()))?;
    let kind = inp.kind.unwrap_or_default();
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::BadRequest(format!("kind must be one of {KINDS:?}")));
    }
    let mut options = inp.options.unwrap_or_default();
    if has_options(&kind) {
        if options.iter().any(|o| o.id.is_some()) {
            return Err(AppError::BadRequest("a new tracker has no existing options".into()));
        }
        check_options(&mut options, &[])?;
    }
    let mut tx = st.db.begin().await?;
    let next_order: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM trackers WHERE patient_id = ?",
    )
    .bind(p.id)
    .fetch_one(&mut *tx)
    .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO trackers (patient_id, name, kind, unit, icon, pinned, sort_order)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(p.id)
    .bind(&name)
    .bind(&kind)
    .bind(clean_opt_text(inp.unit.flatten()))
    .bind(clean_opt_text(inp.icon.flatten()))
    .bind(inp.pinned.unwrap_or(false))
    .bind(next_order)
    .fetch_one(&mut *tx)
    .await?;
    if has_options(&kind) {
        save_options(&mut tx, id, &options, &[]).await?;
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(tracker_with_options(&st, p.id, id).await?)))
}

async fn update_tracker(
    p: Patient,
    State(st): State<AppState>,
    Path((_, id)): Path<(i64, i64)>,
    Json(inp): Json<TrackerInput>,
) -> AppResult<Json<Tracker>> {
    let cur = fetch_tracker(&st, p.id, id).await?;
    if inp.kind.as_deref().is_some_and(|k| k != cur.kind) {
        // Changing the kind would make old entries meaningless.
        return Err(AppError::BadRequest(
            "kind can't be changed; archive this one and create a new tracker".into(),
        ));
    }
    let name = match inp.name {
        Some(n) => clean_opt_text(Some(n)).ok_or(AppError::BadRequest("name is required".into()))?,
        None => cur.name,
    };
    let unit = match inp.unit {
        Some(u) => clean_opt_text(u),
        None => cur.unit,
    };
    let icon = match inp.icon {
        Some(i) => clean_opt_text(i),
        None => cur.icon,
    };
    let mut options = inp.options.filter(|_| has_options(&cur.kind));
    let existing = fetch_options(&st, p.id, Some(id)).await?;
    if let Some(o) = options.as_mut() {
        check_options(o, &existing)?;
    }

    let mut tx = st.db.begin().await?;
    sqlx::query(
        "UPDATE trackers
            SET name = ?, unit = ?, icon = ?, pinned = ?, archived = ?
          WHERE id = ?",
    )
    .bind(name)
    .bind(unit)
    .bind(icon)
    .bind(inp.pinned.unwrap_or(cur.pinned))
    .bind(inp.archived.unwrap_or(cur.archived))
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if let Some(o) = options {
        save_options(&mut tx, id, &o, &existing).await?;
    }
    tx.commit().await?;
    Ok(Json(tracker_with_options(&st, p.id, id).await?))
}

#[derive(Deserialize)]
struct MergeInput {
    into: i64,
}

/// Move all entries of one option to another option of the same tracker,
/// then delete the now unused option. The only operation that rewrites history.
async fn merge_option(
    p: Patient,
    State(st): State<AppState>,
    Path((_, tracker_id, from)): Path<(i64, i64, i64)>,
    Json(inp): Json<MergeInput>,
) -> AppResult<Json<Tracker>> {
    let existing = fetch_options(&st, p.id, Some(tracker_id)).await?;
    let belongs = |id| existing.iter().any(|o| o.id == id);
    if !belongs(from) || !belongs(inp.into) {
        return Err(AppError::NotFound);
    }
    if from == inp.into {
        return Err(AppError::BadRequest("can't merge an option into itself".into()));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE entries SET option_id = ? WHERE option_id = ?")
        .bind(inp.into)
        .bind(from)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM tracker_options WHERE id = ?")
        .bind(from)
        .execute(&mut *tx)
        .await?;
    if existing.iter().any(|o| o.id == from && o.normal) {
        // Merging the normal option: the one it became is the normal one now.
        sqlx::query("UPDATE tracker_options SET normal = 1 WHERE id = ? AND NOT retired")
            .bind(inp.into)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(tracker_with_options(&st, p.id, tracker_id).await?))
}

async fn reorder_trackers(
    p: Patient,
    State(st): State<AppState>,
    Json(ids): Json<Vec<i64>>,
) -> AppResult<StatusCode> {
    let mut tx = st.db.begin().await?;
    for (i, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE trackers SET sort_order = ? WHERE id = ? AND patient_id = ?")
            .bind(i as i64)
            .bind(id)
            .bind(p.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- entries ----------

#[derive(Serialize, sqlx::FromRow)]
struct Entry {
    id: String,
    tracker_id: i64,
    tracker_name: String,
    kind: String,
    /// Set for scale/choice entries; `value` then carries the option's label.
    option_id: Option<i64>,
    value: Option<String>,
    note: Option<String>,
    logged_by: Option<i64>,
    logged_by_name: Option<String>,
    occurred_at: String,
    created_at: String,
}

#[derive(Deserialize)]
struct ListEntriesQ {
    from: Option<String>,
    to: Option<String>,
    tracker_id: Option<i64>,
    limit: Option<i64>,
}

async fn query_entries(st: &AppState, pid: i64, q: &ListEntriesQ) -> AppResult<Vec<Entry>> {
    let from = q.from.as_deref().map(normalize_ts).transpose()?;
    let to = q.to.as_deref().map(normalize_ts).transpose()?;
    let limit = q.limit.unwrap_or(2000).clamp(1, 10_000);
    let rows = sqlx::query_as(
        "SELECT e.id, e.tracker_id, t.name AS tracker_name, t.kind, e.option_id,
                COALESCE(o.label, e.value) AS value, e.note,
                e.logged_by, u.display_name AS logged_by_name, e.occurred_at, e.created_at
           FROM entries e
           JOIN trackers t ON t.id = e.tracker_id
           LEFT JOIN tracker_options o ON o.id = e.option_id
           LEFT JOIN users u ON u.id = e.logged_by
          WHERE t.patient_id = ?5
            AND (?1 IS NULL OR e.occurred_at >= ?1)
            AND (?2 IS NULL OR e.occurred_at <  ?2)
            AND (?3 IS NULL OR e.tracker_id = ?3)
          ORDER BY e.occurred_at DESC
          LIMIT ?4",
    )
    .bind(from)
    .bind(to)
    .bind(q.tracker_id)
    .bind(limit)
    .bind(pid)
    .fetch_all(&st.db)
    .await?;
    Ok(rows)
}

async fn list_entries(
    p: Patient,
    State(st): State<AppState>,
    Query(q): Query<ListEntriesQ>,
) -> AppResult<Json<Vec<Entry>>> {
    Ok(Json(query_entries(&st, p.id, &q).await?))
}

/// An entry of this patient (404 for other patients' entries).
async fn fetch_entry(st: &AppState, pid: i64, id: &str) -> AppResult<Entry> {
    sqlx::query_as(
        "SELECT e.id, e.tracker_id, t.name AS tracker_name, t.kind, e.option_id,
                COALESCE(o.label, e.value) AS value, e.note,
                e.logged_by, u.display_name AS logged_by_name, e.occurred_at, e.created_at
           FROM entries e
           JOIN trackers t ON t.id = e.tracker_id
           LEFT JOIN tracker_options o ON o.id = e.option_id
           LEFT JOIN users u ON u.id = e.logged_by
          WHERE e.id = ? AND t.patient_id = ?",
    )
    .bind(id)
    .bind(pid)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// What an entry stores: a free value, or an option for scale/choice.
#[derive(Debug, PartialEq)]
struct EntryValue {
    value: Option<String>,
    option_id: Option<i64>,
}

/// Check and normalise a value against its tracker's kind. `options` are the
/// tracker's options; retired ones are accepted too, so an entry queued
/// offline still saves if its option was retired in the meantime.
fn validate_value(
    tracker: &TrackerRow,
    options: &[TrackerOption],
    value: Option<String>,
    option_id: Option<i64>,
    note: &Option<String>,
) -> AppResult<EntryValue> {
    let value = clean_opt_text(value);
    let bad = |m: &str| Err(AppError::BadRequest(format!("{}: {m}", tracker.name)));
    let plain = |v: &str| Ok(EntryValue { value: Some(v.to_string()), option_id: None });
    match tracker.kind.as_str() {
        "bool" => plain("yes"),
        "count" => match value {
            None => plain("1"),
            Some(v) => match v.parse::<i64>() {
                Ok(n) if n >= 1 => plain(&n.to_string()),
                _ => bad("count must be a whole number ≥ 1"),
            },
        },
        "scale" | "choice" => match option_id {
            Some(id) if options.iter().any(|o| o.id == id) => {
                Ok(EntryValue { value: None, option_id: Some(id) })
            }
            _ => bad("pick one of the options"),
        },
        "number" => match value.as_deref().map(|v| v.replace(',', ".").parse::<f64>()) {
            Some(Ok(n)) if n.is_finite() => plain(&n.to_string()),
            _ => bad("enter a number"),
        },
        "med" => plain(&value.or_else(|| tracker.unit.clone()).unwrap_or("given".into())),
        "note" => {
            if note.as_deref().is_none_or(|n| n.trim().is_empty()) {
                bad("note can't be empty")
            } else {
                Ok(EntryValue { value: None, option_id: None })
            }
        }
        _ => bad("unknown kind"),
    }
}

/// The id a client generated for something it may re-send, or a new one.
fn parse_client_id(id: Option<String>) -> AppResult<String> {
    match id {
        Some(id) => Ok(uuid::Uuid::parse_str(&id)
            .map_err(|_| AppError::BadRequest("id must be a UUID".into()))?
            .to_string()),
        None => Ok(uuid::Uuid::new_v4().to_string()),
    }
}

#[derive(Deserialize)]
struct EntryInput {
    id: Option<String>,
    tracker_id: i64,
    value: Option<String>,
    option_id: Option<i64>,
    note: Option<String>,
    occurred_at: Option<String>,
}

async fn create_entry(
    p: Patient,
    State(st): State<AppState>,
    Json(inp): Json<EntryInput>,
) -> AppResult<(StatusCode, Json<Entry>)> {
    let tracker = fetch_tracker(&st, p.id, inp.tracker_id).await?;
    let options = fetch_options(&st, p.id, Some(tracker.id)).await?;
    let note = clean_opt_text(inp.note);
    let v = validate_value(&tracker, &options, inp.value, inp.option_id, &note)?;
    let occurred_at = match inp.occurred_at.as_deref() {
        Some(t) => normalize_ts(t)?,
        None => now_ts(),
    };
    let id = parse_client_id(inp.id)?;
    // Idempotent: re-sending a queued offline entry is a no-op.
    sqlx::query(
        "INSERT OR IGNORE INTO entries (id, tracker_id, option_id, value, note, logged_by, occurred_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(tracker.id)
    .bind(v.option_id)
    .bind(v.value)
    .bind(note)
    .bind(p.user.id)
    .bind(occurred_at)
    .execute(&st.db)
    .await?;
    Ok((StatusCode::CREATED, Json(fetch_entry(&st, p.id, &id).await?)))
}

#[derive(Deserialize)]
struct EntryPatch {
    value: Option<String>,
    option_id: Option<i64>,
    note: Option<Option<String>>,
    occurred_at: Option<String>,
}

async fn update_entry(
    pat: Patient,
    State(st): State<AppState>,
    Path((_, id)): Path<(i64, String)>,
    Json(p): Json<EntryPatch>,
) -> AppResult<Json<Entry>> {
    let cur = fetch_entry(&st, pat.id, &id).await?;
    let tracker = fetch_tracker(&st, pat.id, cur.tracker_id).await?;
    let options = fetch_options(&st, pat.id, Some(tracker.id)).await?;
    let note = match p.note {
        Some(n) => clean_opt_text(n),
        None => cur.note,
    };
    // For option entries cur.value is the label, so only fall back to it otherwise.
    let v = if has_options(&tracker.kind) {
        validate_value(&tracker, &options, None, p.option_id.or(cur.option_id), &note)?
    } else {
        validate_value(&tracker, &options, p.value.or(cur.value), None, &note)?
    };
    let occurred_at = match p.occurred_at.as_deref() {
        Some(t) => normalize_ts(t)?,
        None => cur.occurred_at,
    };
    sqlx::query("UPDATE entries SET option_id = ?, value = ?, note = ?, occurred_at = ? WHERE id = ?")
        .bind(v.option_id)
        .bind(v.value)
        .bind(note)
        .bind(occurred_at)
        .bind(&id)
        .execute(&st.db)
        .await?;
    Ok(Json(fetch_entry(&st, pat.id, &id).await?))
}

async fn delete_entry(
    p: Patient,
    State(st): State<AppState>,
    Path((_, id)): Path<(i64, String)>,
) -> AppResult<StatusCode> {
    let res = sqlx::query(
        "DELETE FROM entries
          WHERE id = ? AND tracker_id IN (SELECT id FROM trackers WHERE patient_id = ?)",
    )
    .bind(&id)
    .bind(p.id)
    .execute(&st.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- check-ins ----------

/// "Nothing unusual today": the day was observed, so normal options apply.
#[derive(Serialize, sqlx::FromRow)]
struct Checkin {
    id: String,
    logged_by: Option<i64>,
    logged_by_name: Option<String>,
    occurred_at: String,
    created_at: String,
}

#[derive(Deserialize)]
struct ListCheckinsQ {
    from: Option<String>,
    to: Option<String>,
}

async fn list_checkins(
    p: Patient,
    State(st): State<AppState>,
    Query(q): Query<ListCheckinsQ>,
) -> AppResult<Json<Vec<Checkin>>> {
    let from = q.from.as_deref().map(normalize_ts).transpose()?;
    let to = q.to.as_deref().map(normalize_ts).transpose()?;
    Ok(Json(
        sqlx::query_as(
            "SELECT c.id, c.logged_by, u.display_name AS logged_by_name, c.occurred_at, c.created_at
               FROM checkins c LEFT JOIN users u ON u.id = c.logged_by
              WHERE c.patient_id = ?1
                AND (?2 IS NULL OR c.occurred_at >= ?2)
                AND (?3 IS NULL OR c.occurred_at <  ?3)
              ORDER BY c.occurred_at DESC
              LIMIT 2000",
        )
        .bind(p.id)
        .bind(from)
        .bind(to)
        .fetch_all(&st.db)
        .await?,
    ))
}

#[derive(Deserialize)]
struct CheckinInput {
    id: Option<String>,
    occurred_at: Option<String>,
}

async fn create_checkin(
    p: Patient,
    State(st): State<AppState>,
    Json(inp): Json<CheckinInput>,
) -> AppResult<(StatusCode, Json<Checkin>)> {
    let id = parse_client_id(inp.id)?;
    let occurred_at = match inp.occurred_at.as_deref() {
        Some(t) => normalize_ts(t)?,
        None => now_ts(),
    };
    // Idempotent like entries, for the offline queue.
    sqlx::query("INSERT OR IGNORE INTO checkins (id, patient_id, logged_by, occurred_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(p.id)
        .bind(p.user.id)
        .bind(occurred_at)
        .execute(&st.db)
        .await?;
    let saved = sqlx::query_as(
        "SELECT c.id, c.logged_by, u.display_name AS logged_by_name, c.occurred_at, c.created_at
           FROM checkins c LEFT JOIN users u ON u.id = c.logged_by
          WHERE c.id = ? AND c.patient_id = ?",
    )
        .bind(&id)
        .bind(p.id)
        .fetch_optional(&st.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok((StatusCode::CREATED, Json(saved)))
}

async fn delete_checkin(
    p: Patient,
    State(st): State<AppState>,
    Path((_, id)): Path<(i64, String)>,
) -> AppResult<StatusCode> {
    let res = sqlx::query("DELETE FROM checkins WHERE id = ? AND patient_id = ?")
        .bind(&id)
        .bind(p.id)
        .execute(&st.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(kind: &str, unit: Option<&str>) -> TrackerRow {
        TrackerRow {
            id: 1,
            name: "X".into(),
            kind: kind.into(),
            unit: unit.map(Into::into),
            icon: None,
            pinned: false,
            archived: false,
            sort_order: 0,
            created_at: String::new(),
        }
    }

    fn opt(id: i64, label: &str, retired: bool, uses: i64) -> TrackerOption {
        TrackerOption { id, tracker_id: 1, label: label.into(), retired, normal: false, uses }
    }

    fn input(id: Option<i64>, label: &str, retired: bool) -> OptionInput {
        OptionInput { id, label: label.into(), retired, normal: false }
    }

    #[test]
    fn validates_values_by_kind() {
        let none = None;
        let val = |v: &str| EntryValue { value: Some(v.into()), option_id: None };
        assert_eq!(validate_value(&tracker("bool", None), &[], None, None, &none).unwrap(), val("yes"));
        assert_eq!(validate_value(&tracker("count", None), &[], None, None, &none).unwrap(), val("1"));
        assert!(validate_value(&tracker("count", None), &[], Some("0".into()), None, &none).is_err());
        let num = tracker("number", Some("kg"));
        assert_eq!(validate_value(&num, &[], Some("4,25".into()), None, &none).unwrap(), val("4.25"));
        assert!(validate_value(&num, &[], Some("abc".into()), None, &none).is_err());
        let med = tracker("med", Some("1/4 tab"));
        assert_eq!(validate_value(&med, &[], None, None, &none).unwrap(), val("1/4 tab"));
        assert!(validate_value(&tracker("note", None), &[], None, None, &none).is_err());
        assert!(validate_value(&tracker("note", None), &[], None, None, &Some("ok".into())).is_ok());
    }

    #[test]
    fn option_entries_need_one_of_the_trackers_options() {
        let none = None;
        let scale = tracker("scale", None);
        let opts = [opt(10, "little", false, 0), opt(11, "soft", true, 3)];
        let picked = validate_value(&scale, &opts, None, Some(10), &none).unwrap();
        assert_eq!(picked, EntryValue { value: None, option_id: Some(10) });
        // Retired options still validate (offline queue), foreign ones don't.
        assert!(validate_value(&scale, &opts, None, Some(11), &none).is_ok());
        assert!(validate_value(&scale, &opts, None, Some(99), &none).is_err());
        assert!(validate_value(&scale, &opts, Some("little".into()), None, &none).is_err());
    }

    #[test]
    fn checks_option_lists() {
        let existing = [opt(1, "soft", false, 4), opt(2, "hard", false, 0)];
        // soft/hard -> retire soft, rename hard, add new ones.
        let mut ok = vec![
            input(Some(1), "soft", true),
            input(Some(2), " stone ", false),
            input(None, "porridge", false),
        ];
        assert!(check_options(&mut ok, &existing).is_ok());
        assert_eq!(ok[1].label, "stone");
        let mut dup = vec![input(Some(1), "Soft", false), input(None, "soft", false)];
        assert!(check_options(&mut dup, &existing).is_err());
        let mut one_active = vec![input(Some(1), "soft", true), input(Some(2), "hard", false)];
        assert!(check_options(&mut one_active, &existing).is_err());
        let mut foreign = vec![input(Some(7), "x", false), input(None, "y", false)];
        assert!(check_options(&mut foreign, &existing).is_err());
        let mut empty = vec![input(Some(1), "  ", false), input(None, "y", false)];
        assert!(check_options(&mut empty, &existing).is_err());

        let normal = |o: OptionInput| OptionInput { normal: true, ..o };
        let mut one_normal = vec![normal(input(Some(1), "soft", false)), input(Some(2), "hard", false)];
        assert!(check_options(&mut one_normal, &existing).is_ok());
        let mut two_normal = vec![normal(input(Some(1), "soft", false)), normal(input(Some(2), "hard", false))];
        assert!(check_options(&mut two_normal, &existing).is_err());
        let mut retired_normal = vec![
            normal(input(Some(1), "soft", true)),
            input(Some(2), "hard", false),
            input(None, "stone", false),
        ];
        assert!(check_options(&mut retired_normal, &existing).is_err());
    }

}
