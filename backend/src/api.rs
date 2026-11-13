//! JSON API under /api.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, patch, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    AppState,
    auth::{self, CurrentUser},
    error::{AppError, AppResult},
    normalize_ts, now_ts,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/symptoms", get(list_symptoms).post(create_symptom))
        .route("/symptoms/order", put(reorder_symptoms))
        .route("/symptoms/{id}", patch(update_symptom))
        .route("/entries", get(list_entries).post(create_entry))
        .route("/entries/{id}", patch(update_entry).delete(delete_entry))
        .route("/export.csv", get(export_csv))
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

// ---------- symptom types ----------

pub const KINDS: &[&str] = &["bool", "count", "scale", "choice", "number", "med", "note"];

#[derive(sqlx::FromRow)]
struct SymptomRow {
    id: i64,
    name: String,
    kind: String,
    options: Option<String>,
    unit: Option<String>,
    icon: Option<String>,
    pinned: bool,
    archived: bool,
    sort_order: i64,
}

#[derive(Serialize)]
pub struct Symptom {
    id: i64,
    name: String,
    kind: String,
    options: Vec<String>,
    unit: Option<String>,
    icon: Option<String>,
    pinned: bool,
    archived: bool,
    sort_order: i64,
}

impl From<SymptomRow> for Symptom {
    fn from(r: SymptomRow) -> Self {
        Symptom {
            id: r.id,
            name: r.name,
            kind: r.kind,
            options: r
                .options
                .and_then(|o| serde_json::from_str(&o).ok())
                .unwrap_or_default(),
            unit: r.unit,
            icon: r.icon,
            pinned: r.pinned,
            archived: r.archived,
            sort_order: r.sort_order,
        }
    }
}

async fn fetch_symptom(st: &AppState, id: i64) -> AppResult<SymptomRow> {
    sqlx::query_as(
        "SELECT id, name, kind, options, unit, icon, pinned, archived, sort_order
           FROM symptom_types WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
struct ListSymptomsQ {
    #[serde(default)]
    include_archived: bool,
}

async fn list_symptoms(
    _u: CurrentUser,
    State(st): State<AppState>,
    Query(q): Query<ListSymptomsQ>,
) -> AppResult<Json<Vec<Symptom>>> {
    let rows: Vec<SymptomRow> = sqlx::query_as(
        "SELECT id, name, kind, options, unit, icon, pinned, archived, sort_order
           FROM symptom_types
          WHERE (? OR archived = 0)
          ORDER BY sort_order, id",
    )
    .bind(q.include_archived)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[derive(Deserialize)]
struct SymptomInput {
    name: Option<String>,
    kind: Option<String>,
    options: Option<Vec<String>>,
    unit: Option<Option<String>>,
    icon: Option<Option<String>>,
    pinned: Option<bool>,
    archived: Option<bool>,
}

fn clean_options(kind: &str, options: Option<Vec<String>>) -> AppResult<Option<String>> {
    if kind == "scale" || kind == "choice" {
        let opts: Vec<String> = options
            .unwrap_or_default()
            .into_iter()
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect();
        if opts.len() < 2 {
            return Err(AppError::BadRequest(
                "scale and choice need at least two options".into(),
            ));
        }
        Ok(Some(serde_json::to_string(&opts).map_err(anyhow::Error::from)?))
    } else {
        Ok(None)
    }
}

fn clean_opt_text(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

async fn create_symptom(
    _u: CurrentUser,
    State(st): State<AppState>,
    Json(inp): Json<SymptomInput>,
) -> AppResult<(StatusCode, Json<Symptom>)> {
    let name = clean_opt_text(inp.name).ok_or(AppError::BadRequest("name is required".into()))?;
    let kind = inp.kind.unwrap_or_default();
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::BadRequest(format!("kind must be one of {KINDS:?}")));
    }
    let options = clean_options(&kind, inp.options)?;
    let next_order: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM symptom_types")
            .fetch_one(&st.db)
            .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO symptom_types (name, kind, options, unit, icon, pinned, sort_order)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&name)
    .bind(&kind)
    .bind(options)
    .bind(clean_opt_text(inp.unit.flatten()))
    .bind(clean_opt_text(inp.icon.flatten()))
    .bind(inp.pinned.unwrap_or(false))
    .bind(next_order)
    .fetch_one(&st.db)
    .await?;
    Ok((StatusCode::CREATED, Json(fetch_symptom(&st, id).await?.into())))
}

async fn update_symptom(
    _u: CurrentUser,
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Json(inp): Json<SymptomInput>,
) -> AppResult<Json<Symptom>> {
    let cur = fetch_symptom(&st, id).await?;
    if inp.kind.as_deref().is_some_and(|k| k != cur.kind) {
        // Changing the kind would make old entries meaningless.
        return Err(AppError::BadRequest(
            "kind can't be changed; archive this one and create a new symptom".into(),
        ));
    }
    let name = match inp.name {
        Some(n) => clean_opt_text(Some(n)).ok_or(AppError::BadRequest("name is required".into()))?,
        None => cur.name,
    };
    let options = match inp.options {
        Some(o) => clean_options(&cur.kind, Some(o))?,
        None => cur.options,
    };
    let unit = match inp.unit {
        Some(u) => clean_opt_text(u),
        None => cur.unit,
    };
    let icon = match inp.icon {
        Some(i) => clean_opt_text(i),
        None => cur.icon,
    };
    sqlx::query(
        "UPDATE symptom_types
            SET name = ?, options = ?, unit = ?, icon = ?, pinned = ?, archived = ?
          WHERE id = ?",
    )
    .bind(name)
    .bind(options)
    .bind(unit)
    .bind(icon)
    .bind(inp.pinned.unwrap_or(cur.pinned))
    .bind(inp.archived.unwrap_or(cur.archived))
    .bind(id)
    .execute(&st.db)
    .await?;
    Ok(Json(fetch_symptom(&st, id).await?.into()))
}

async fn reorder_symptoms(
    _u: CurrentUser,
    State(st): State<AppState>,
    Json(ids): Json<Vec<i64>>,
) -> AppResult<StatusCode> {
    let mut tx = st.db.begin().await?;
    for (i, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE symptom_types SET sort_order = ? WHERE id = ?")
            .bind(i as i64)
            .bind(id)
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
    symptom_id: i64,
    symptom_name: String,
    kind: String,
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
    symptom_id: Option<i64>,
    limit: Option<i64>,
}

async fn query_entries(st: &AppState, q: &ListEntriesQ) -> AppResult<Vec<Entry>> {
    let from = q.from.as_deref().map(normalize_ts).transpose()?;
    let to = q.to.as_deref().map(normalize_ts).transpose()?;
    let limit = q.limit.unwrap_or(2000).clamp(1, 10_000);
    let rows = sqlx::query_as(
        "SELECT e.id, e.symptom_id, s.name AS symptom_name, s.kind, e.value, e.note,
                e.logged_by, u.display_name AS logged_by_name, e.occurred_at, e.created_at
           FROM entries e
           JOIN symptom_types s ON s.id = e.symptom_id
           LEFT JOIN users u ON u.id = e.logged_by
          WHERE (?1 IS NULL OR e.occurred_at >= ?1)
            AND (?2 IS NULL OR e.occurred_at <  ?2)
            AND (?3 IS NULL OR e.symptom_id = ?3)
          ORDER BY e.occurred_at DESC
          LIMIT ?4",
    )
    .bind(from)
    .bind(to)
    .bind(q.symptom_id)
    .bind(limit)
    .fetch_all(&st.db)
    .await?;
    Ok(rows)
}

async fn list_entries(
    _u: CurrentUser,
    State(st): State<AppState>,
    Query(q): Query<ListEntriesQ>,
) -> AppResult<Json<Vec<Entry>>> {
    Ok(Json(query_entries(&st, &q).await?))
}

async fn fetch_entry(st: &AppState, id: &str) -> AppResult<Entry> {
    sqlx::query_as(
        "SELECT e.id, e.symptom_id, s.name AS symptom_name, s.kind, e.value, e.note,
                e.logged_by, u.display_name AS logged_by_name, e.occurred_at, e.created_at
           FROM entries e
           JOIN symptom_types s ON s.id = e.symptom_id
           LEFT JOIN users u ON u.id = e.logged_by
          WHERE e.id = ?",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// Check and normalise a value against its symptom's kind.
fn validate_value(
    sym: &SymptomRow,
    value: Option<String>,
    note: &Option<String>,
) -> AppResult<Option<String>> {
    let value = clean_opt_text(value);
    let bad = |m: &str| Err(AppError::BadRequest(format!("{}: {m}", sym.name)));
    match sym.kind.as_str() {
        "bool" => Ok(Some("yes".into())),
        "count" => match value {
            None => Ok(Some("1".into())),
            Some(v) => match v.parse::<i64>() {
                Ok(n) if n >= 1 => Ok(Some(n.to_string())),
                _ => bad("count must be a whole number ≥ 1"),
            },
        },
        "scale" | "choice" => {
            let opts: Vec<String> = sym
                .options
                .as_deref()
                .and_then(|o| serde_json::from_str(o).ok())
                .unwrap_or_default();
            match value {
                Some(v) if opts.contains(&v) => Ok(Some(v)),
                _ => bad(&format!("pick one of {opts:?}")),
            }
        }
        "number" => match value.as_deref().map(|v| v.replace(',', ".").parse::<f64>()) {
            Some(Ok(n)) if n.is_finite() => Ok(Some(n.to_string())),
            _ => bad("enter a number"),
        },
        "med" => Ok(value.or_else(|| sym.unit.clone()).or(Some("given".into()))),
        "note" => {
            if note.as_deref().is_none_or(|n| n.trim().is_empty()) {
                bad("note can't be empty")
            } else {
                Ok(None)
            }
        }
        _ => bad("unknown kind"),
    }
}

#[derive(Deserialize)]
struct EntryInput {
    id: Option<String>,
    symptom_id: i64,
    value: Option<String>,
    note: Option<String>,
    occurred_at: Option<String>,
}

async fn create_entry(
    user: CurrentUser,
    State(st): State<AppState>,
    Json(inp): Json<EntryInput>,
) -> AppResult<(StatusCode, Json<Entry>)> {
    let sym = fetch_symptom(&st, inp.symptom_id).await?;
    let note = clean_opt_text(inp.note);
    let value = validate_value(&sym, inp.value, &note)?;
    let occurred_at = match inp.occurred_at.as_deref() {
        Some(t) => normalize_ts(t)?,
        None => now_ts(),
    };
    let id = match inp.id {
        Some(id) => uuid::Uuid::parse_str(&id)
            .map_err(|_| AppError::BadRequest("id must be a UUID".into()))?
            .to_string(),
        None => uuid::Uuid::new_v4().to_string(),
    };
    // Idempotent: re-sending a queued offline entry is a no-op.
    sqlx::query(
        "INSERT OR IGNORE INTO entries (id, symptom_id, value, note, logged_by, occurred_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(sym.id)
    .bind(value)
    .bind(note)
    .bind(user.id)
    .bind(occurred_at)
    .execute(&st.db)
    .await?;
    Ok((StatusCode::CREATED, Json(fetch_entry(&st, &id).await?)))
}

#[derive(Deserialize)]
struct EntryPatch {
    value: Option<String>,
    note: Option<Option<String>>,
    occurred_at: Option<String>,
}

async fn update_entry(
    _u: CurrentUser,
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(p): Json<EntryPatch>,
) -> AppResult<Json<Entry>> {
    let cur = fetch_entry(&st, &id).await?;
    let sym = fetch_symptom(&st, cur.symptom_id).await?;
    let note = match p.note {
        Some(n) => clean_opt_text(n),
        None => cur.note,
    };
    let value = validate_value(&sym, p.value.or(cur.value), &note)?;
    let occurred_at = match p.occurred_at.as_deref() {
        Some(t) => normalize_ts(t)?,
        None => cur.occurred_at,
    };
    sqlx::query("UPDATE entries SET value = ?, note = ?, occurred_at = ? WHERE id = ?")
        .bind(value)
        .bind(note)
        .bind(occurred_at)
        .bind(&id)
        .execute(&st.db)
        .await?;
    Ok(Json(fetch_entry(&st, &id).await?))
}

async fn delete_entry(
    _u: CurrentUser,
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let res = sqlx::query("DELETE FROM entries WHERE id = ?")
        .bind(&id)
        .execute(&st.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- export ----------

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

async fn export_csv(
    _u: CurrentUser,
    State(st): State<AppState>,
    Query(q): Query<ListEntriesQ>,
) -> AppResult<impl IntoResponse> {
    let q = ListEntriesQ {
        limit: Some(q.limit.unwrap_or(10_000)),
        ..q
    };
    let rows = query_entries(&st, &q).await?;
    let mut out = String::from("occurred_at,symptom,value,note,logged_by\n");
    for e in rows.iter().rev() {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            e.occurred_at,
            csv_field(&e.symptom_name),
            csv_field(e.value.as_deref().unwrap_or("")),
            csv_field(e.note.as_deref().unwrap_or("")),
            csv_field(e.logged_by_name.as_deref().unwrap_or("")),
        ));
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"cat-tracker-export.csv\"",
            ),
        ],
        out,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(kind: &str, options: Option<&str>, unit: Option<&str>) -> SymptomRow {
        SymptomRow {
            id: 1,
            name: "X".into(),
            kind: kind.into(),
            options: options.map(Into::into),
            unit: unit.map(Into::into),
            icon: None,
            pinned: false,
            archived: false,
            sort_order: 0,
        }
    }

    #[test]
    fn validates_values_by_kind() {
        let none = None;
        assert_eq!(validate_value(&sym("bool", None, None), None, &none).unwrap(), Some("yes".into()));
        assert_eq!(validate_value(&sym("count", None, None), None, &none).unwrap(), Some("1".into()));
        assert!(validate_value(&sym("count", None, None), Some("0".into()), &none).is_err());
        let scale = sym("scale", Some(r#"["none","little","normal"]"#), None);
        assert!(validate_value(&scale, Some("little".into()), &none).is_ok());
        assert!(validate_value(&scale, Some("lots".into()), &none).is_err());
        let num = sym("number", None, Some("kg"));
        assert_eq!(validate_value(&num, Some("4,25".into()), &none).unwrap(), Some("4.25".into()));
        assert!(validate_value(&num, Some("abc".into()), &none).is_err());
        let med = sym("med", None, Some("1/4 tab"));
        assert_eq!(validate_value(&med, None, &none).unwrap(), Some("1/4 tab".into()));
        assert!(validate_value(&sym("note", None, None), None, &none).is_err());
        assert!(validate_value(&sym("note", None, None), None, &Some("ok".into())).is_ok());
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
