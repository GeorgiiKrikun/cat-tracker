//! Patients (whoever is observed) and which users can see them.

use anyhow::Context;
use axum::{
    Json,
    extract::{FromRequestParts, RawPathParams, State},
    http::request::Parts,
};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::{
    AppState,
    auth::CurrentUser,
    error::{AppError, AppResult},
};

/// Extractor for routes under `/patients/{pid}/…`: a logged-in user who has
/// access to that patient. Without access the patient "doesn't exist" (404),
/// so other people's patients aren't revealed.
pub struct Patient {
    pub id: i64,
    pub user: CurrentUser,
}

impl FromRequestParts<AppState> for Patient {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        let params = RawPathParams::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::NotFound)?;
        let id: i64 = params
            .iter()
            .find(|(k, _)| *k == "pid")
            .and_then(|(_, v)| v.parse().ok())
            .ok_or(AppError::NotFound)?;
        let allowed: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM patient_access WHERE patient_id = ? AND user_id = ?")
                .bind(id)
                .bind(user.id)
                .fetch_optional(&state.db)
                .await?;
        allowed.ok_or(AppError::NotFound)?;
        Ok(Patient { id, user })
    }
}

#[derive(Serialize)]
pub struct PatientInfo {
    id: i64,
    name: String,
    icon: Option<String>,
    /// Display names of the other users who can see this patient.
    shared_with: Vec<String>,
}

/// GET /api/patients: the patients the current user can see.
pub async fn list(user: CurrentUser, State(st): State<AppState>) -> AppResult<Json<Vec<PatientInfo>>> {
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT p.id, p.name, p.icon,
                (SELECT json_group_array(u.display_name)
                   FROM patient_access o JOIN users u ON u.id = o.user_id
                  WHERE o.patient_id = p.id AND o.user_id != a.user_id)
           FROM patients p
           JOIN patient_access a ON a.patient_id = p.id
          WHERE a.user_id = ?
          ORDER BY p.id",
    )
    .bind(user.id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, icon, shared)| PatientInfo {
                id,
                name,
                icon,
                shared_with: shared
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
            })
            .collect(),
    ))
}

// ---------- presets ----------

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Preset {
    /// No symptoms; add them in the app's Settings.
    None,
    /// Cat starter set: meds, appetite, vomiting, stool, drinking, weight…
    Cat,
}

struct PresetSymptom {
    name: &'static str,
    kind: &'static str,
    unit: Option<&'static str>,
    icon: &'static str,
    pinned: bool,
    /// Scale options worst/least first.
    options: &'static [&'static str],
}

const fn sym(
    name: &'static str,
    kind: &'static str,
    icon: &'static str,
    pinned: bool,
    options: &'static [&'static str],
) -> PresetSymptom {
    PresetSymptom { name, kind, unit: None, icon, pinned, options }
}

const CAT: &[PresetSymptom] = &[
    sym("Omeprazole", "med", "💊", true, &[]),
    sym("Emeprid", "med", "💊", true, &[]),
    sym("Pronefra", "med", "💊", true, &[]),
    sym("Prednisolone", "med", "💊", true, &[]),
    sym("Appetite", "scale", "🍽️", true, &["none", "little", "normal", "good"]),
    sym("Vomited", "count", "🤮", true, &[]),
    sym("Stool", "choice", "💩", true, &["normal", "soft", "diarrhea", "hard", "with blood"]),
    sym("Drinking", "scale", "💧", true, &["less", "normal", "more", "a lot"]),
    sym("Peeing", "scale", "🚽", false, &["less", "normal", "more", "a lot"]),
    sym("Night waking", "scale", "🌙", true, &["mild", "moderate", "intense"]),
    sym("Energy", "scale", "⚡", false, &["very low", "low", "normal", "playful"]),
    PresetSymptom { unit: Some("kg"), ..sym("Weight", "number", "⚖️", false, &[]) },
    PresetSymptom { unit: Some("mmol/L"), ..sym("Blood glucose", "number", "🩸", false, &[]) },
    sym("Note", "note", "📝", true, &[]),
];

impl Preset {
    fn symptoms(self) -> &'static [PresetSymptom] {
        match self {
            Preset::None => &[],
            Preset::Cat => CAT,
        }
    }
}

// ---------- CLI helpers ----------

pub async fn create(db: &SqlitePool, name: &str, icon: Option<&str>, preset: Preset) -> anyhow::Result<i64> {
    let mut tx = db.begin().await?;
    let pid: i64 = sqlx::query_scalar("INSERT INTO patients (name, icon) VALUES (?, ?) RETURNING id")
        .bind(name)
        .bind(icon)
        .fetch_one(&mut *tx)
        .await?;
    for (i, s) in preset.symptoms().iter().enumerate() {
        let sid: i64 = sqlx::query_scalar(
            "INSERT INTO symptom_types (patient_id, name, kind, unit, icon, pinned, sort_order)
             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(pid)
        .bind(s.name)
        .bind(s.kind)
        .bind(s.unit)
        .bind(s.icon)
        .bind(s.pinned)
        .bind(i as i64)
        .fetch_one(&mut *tx)
        .await?;
        for (j, label) in s.options.iter().enumerate() {
            sqlx::query("INSERT INTO symptom_options (symptom_id, label, sort_order) VALUES (?, ?, ?)")
                .bind(sid)
                .bind(label)
                .bind(j as i64)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(pid)
}

/// Find a patient by id or by (unique) name.
pub async fn resolve(db: &SqlitePool, patient: &str) -> anyhow::Result<(i64, String)> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM patients WHERE CAST(id AS TEXT) = ?1 OR name = ?1 COLLATE NOCASE")
            .bind(patient)
            .fetch_all(db)
            .await?;
    match rows.as_slice() {
        [one] => Ok(one.clone()),
        [] => anyhow::bail!("no patient {patient:?} (see `cattracker patients`)"),
        _ => anyhow::bail!("several patients are called {patient:?}; use the id (see `cattracker patients`)"),
    }
}

async fn user_id(db: &SqlitePool, username: &str) -> anyhow::Result<i64> {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(db)
        .await?
        .with_context(|| format!("no such user: {username}"))
}

pub async fn share(db: &SqlitePool, patient_id: i64, username: &str) -> anyhow::Result<()> {
    let uid = user_id(db, username).await?;
    sqlx::query("INSERT OR IGNORE INTO patient_access (patient_id, user_id) VALUES (?, ?)")
        .bind(patient_id)
        .bind(uid)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn unshare(db: &SqlitePool, patient_id: i64, username: &str) -> anyhow::Result<bool> {
    let uid = user_id(db, username).await?;
    let res = sqlx::query("DELETE FROM patient_access WHERE patient_id = ? AND user_id = ?")
        .bind(patient_id)
        .bind(uid)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Rows of (id, name, icon, comma-separated usernames with access).
pub async fn overview(db: &SqlitePool) -> anyhow::Result<Vec<(i64, String, Option<String>, Option<String>)>> {
    Ok(sqlx::query_as(
        "SELECT p.id, p.name, p.icon,
                (SELECT group_concat(u.username, ', ')
                   FROM patient_access a JOIN users u ON u.id = a.user_id
                  WHERE a.patient_id = p.id)
           FROM patients p ORDER BY p.id",
    )
    .fetch_all(db)
    .await?)
}
