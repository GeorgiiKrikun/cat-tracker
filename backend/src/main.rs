mod api;
mod auth;
mod error;

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Context;
use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::Response,
};
use chrono::{DateTime, SecondsFormat, Utc};
use clap::{Parser, Subcommand};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::error::AppError;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub cookie_secure: bool,
}

/// Current time as a sortable UTC string, e.g. 2026-11-13T10:22:01.123Z
pub fn now_ts() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Accept any RFC 3339 timestamp and store it in the canonical UTC format,
/// so string comparison in SQLite orders correctly.
pub fn normalize_ts(s: &str) -> Result<String, AppError> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| {
            d.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .map_err(|_| AppError::BadRequest(format!("invalid timestamp: {s}")))
}

#[derive(Parser)]
#[command(name = "cattracker", about = "Cat symptom tracker server")]
struct Cli {
    /// Path to the SQLite database file (created if missing).
    #[arg(long, env = "CT_DB_PATH", default_value = "data/cattracker.db", global = true)]
    db: PathBuf,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Parser)]
struct ServeArgs {
    #[arg(long, env = "CT_BIND", default_value = "0.0.0.0:8080")]
    bind: SocketAddr,
    /// Directory with the built PWA (frontend/dist).
    #[arg(long, env = "CT_STATIC_DIR", default_value = "../frontend/dist")]
    static_dir: PathBuf,
    /// Mark cookies Secure. Keep true behind HTTPS (Nginx Proxy Manager);
    /// set false only for plain-http local development.
    #[arg(long, env = "CT_COOKIE_SECURE", default_value_t = true, action = clap::ArgAction::Set)]
    cookie_secure: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the web server (default).
    Serve(ServeArgs),
    /// Create a user. Password is prompted, or read from CT_PASSWORD.
    AddUser {
        username: String,
        /// Name shown next to entries, e.g. "Georgii".
        #[arg(long)]
        display_name: Option<String>,
    },
    /// Change a user's password (logs out their sessions).
    Passwd { username: String },
    /// Delete a user (their entries are kept, shown without a name).
    DelUser { username: String },
    /// List users.
    Users,
    /// Local development only: add a `demo`/`demo` user and two weeks of fake entries.
    SeedDemo,
}

async fn open_db(path: &PathBuf) -> anyhow::Result<SqlitePool> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await
        .with_context(|| format!("opening database {}", path.display()))?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

fn read_password(prompt: &str) -> anyhow::Result<String> {
    if let Ok(p) = std::env::var("CT_PASSWORD") {
        return Ok(p);
    }
    let p1 = rpassword::prompt_password(prompt)?;
    let p2 = rpassword::prompt_password("Repeat: ")?;
    anyhow::ensure!(p1 == p2, "passwords don't match");
    anyhow::ensure!(p1.len() >= 8, "use at least 8 characters");
    Ok(p1)
}

/// Long-lived caching for hashed build assets, no caching for the app shell
/// and service worker so updates show up promptly.
async fn cache_headers(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let mut res = next.run(req).await;
    let value = if path.starts_with("/api/") {
        "no-store"
    } else if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    res.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    res
}

pub fn app(state: AppState, static_dir: PathBuf) -> Router {
    let index = static_dir.join("index.html");
    let spa = ServeDir::new(&static_dir).fallback(ServeFile::new(index));
    Router::new()
        .nest("/api", api::router())
        .fallback_service(spa)
        .layer(middleware::from_fn(cache_headers))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn serve(db: SqlitePool, bind: SocketAddr, static_dir: PathBuf, cookie_secure: bool) -> anyhow::Result<()> {
    if !static_dir.join("index.html").exists() {
        tracing::warn!(
            "no index.html in {} — build the frontend (cd frontend && npm run build) or use the Vite dev server",
            static_dir.display()
        );
    }
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&db).await?;
    if users == 0 {
        tracing::warn!("no users yet — create one with: cattracker add-user <name>");
    }
    let state = AppState { db, cookie_secure };
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("listening on http://{bind} (cookie_secure={cookie_secure})");
    axum::serve(listener, app(state, static_dir))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn seed_demo(db: &SqlitePool) -> anyhow::Result<()> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = 'demo'")
        .fetch_optional(db)
        .await?;
    let uid = match exists {
        Some(id) => id,
        None => sqlx::query_scalar(
            "INSERT INTO users (username, display_name, password_hash) VALUES ('demo', 'Demo', ?) RETURNING id",
        )
        .bind(auth::hash_password("demo")?)
        .fetch_one(db)
        .await?,
    };

    let syms: Vec<(i64, String, String, Option<String>)> =
        sqlx::query_as("SELECT id, name, kind, options FROM symptom_types WHERE archived = 0")
            .fetch_all(db)
            .await?;

    // Small deterministic PRNG so demo data is the same every time.
    let mut seed: u64 = 42;
    let mut rnd = move |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };

    let mut count = 0;
    for day in 0..14i64 {
        let base = Utc::now() - chrono::Duration::days(day);
        for (id, name, kind, options) in &syms {
            let opts: Vec<String> = options
                .as_deref()
                .and_then(|o| serde_json::from_str(o).ok())
                .unwrap_or_default();
            let (times, value): (u64, Option<String>) = match kind.as_str() {
                "med" => (if name == "Omeprazole" { 2 } else { 1 }, Some("given".into())),
                "scale" | "choice" if !opts.is_empty() => {
                    // Bias towards the middle options.
                    let i = ((rnd(opts.len() as u64) + rnd(opts.len() as u64)) / 2) as usize;
                    (1 + rnd(2), Some(opts[i.min(opts.len() - 1)].clone()))
                }
                "count" => (rnd(3).saturating_sub(1), Some("1".into())),
                "number" if name == "Weight" && day % 7 == 0 => {
                    (1, Some(format!("{:.2}", 4.1 + (day as f64) * 0.01)))
                }
                _ => (0, None),
            };
            for k in 0..times {
                let at = base
                    - chrono::Duration::hours(2 + (k as i64) * 6)
                    - chrono::Duration::minutes(rnd(50) as i64);
                sqlx::query(
                    "INSERT INTO entries (id, symptom_id, value, logged_by, occurred_at) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(id)
                .bind(&value)
                .bind(uid)
                .bind(at.to_rfc3339_opts(SecondsFormat::Millis, true))
                .execute(db)
                .await?;
                count += 1;
            }
        }
    }
    println!("Seeded {count} demo entries. Log in with demo / demo (local development only!).");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cattracker=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let db = open_db(&cli.db).await?;

    match cli.cmd.unwrap_or_else(|| Cmd::Serve(ServeArgs::parse_from(["serve"]))) {
        Cmd::Serve(a) => serve(db, a.bind, a.static_dir, a.cookie_secure).await?,
        Cmd::AddUser { username, display_name } => {
            let pw = read_password(&format!("Password for {username}: "))?;
            let hash = auth::hash_password(&pw)?;
            let display = display_name.unwrap_or_else(|| username.clone());
            sqlx::query("INSERT INTO users (username, display_name, password_hash) VALUES (?, ?, ?)")
                .bind(&username)
                .bind(&display)
                .bind(hash)
                .execute(&db)
                .await
                .context("creating user (does it already exist?)")?;
            println!("Created user {username} ({display}).");
        }
        Cmd::Passwd { username } => {
            let pw = read_password(&format!("New password for {username}: "))?;
            let hash = auth::hash_password(&pw)?;
            let res = sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
                .bind(hash)
                .bind(&username)
                .execute(&db)
                .await?;
            anyhow::ensure!(res.rows_affected() == 1, "no such user: {username}");
            sqlx::query("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE username = ?)")
                .bind(&username)
                .execute(&db)
                .await?;
            println!("Password changed; {username} was logged out everywhere.");
        }
        Cmd::DelUser { username } => {
            let res = sqlx::query("DELETE FROM users WHERE username = ?")
                .bind(&username)
                .execute(&db)
                .await?;
            anyhow::ensure!(res.rows_affected() == 1, "no such user: {username}");
            println!("Deleted {username}.");
        }
        Cmd::Users => {
            let rows: Vec<(String, String, String)> =
                sqlx::query_as("SELECT username, display_name, created_at FROM users ORDER BY id")
                    .fetch_all(&db)
                    .await?;
            for (u, d, c) in rows {
                println!("{u:<16} {d:<20} created {c}");
            }
        }
        Cmd::SeedDemo => seed_demo(&db).await?,
    }
    Ok(())
}
