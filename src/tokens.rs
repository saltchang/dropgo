//! Personal API tokens: created and revoked on the tokens page, checked on every `/api/v1` request.

use askama::Template;
use axum::{
    Extension, Form,
    extract::{Path, State},
    http::{HeaderValue, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use tower_sessions::Session;
use uuid::Uuid;

use crate::{
    AppState,
    admin::{Layout, html, internal, layout, rfc3339, utc_text},
    auth::OwnerId,
};

pub const PREFIX: &str = "dgo_";
pub const EXPIRY_DAYS: [i64; 4] = [7, 30, 90, 365];

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Creates a token and returns it in full. Only its hash is stored.
pub async fn create(db: &sqlx::PgPool, user: Uuid, name: &str, days: i64) -> sqlx::Result<String> {
    let token = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(rand::rng().random::<[u8; 32]>()));
    sqlx::query("INSERT INTO api_tokens (id, user_id, name, prefix, hash, expires_at) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(Uuid::new_v4())
        .bind(user)
        .bind(name)
        .bind(&token[PREFIX.len()..PREFIX.len() + 8])
        .bind(hash(&token))
        .bind(OffsetDateTime::now_utc() + Duration::days(days))
        .execute(db)
        .await?;
    Ok(token)
}

/// The owner of a token, as the API sees it.
#[derive(Clone)]
pub struct Caller {
    pub user_id: Uuid,
    pub name: String,
    pub expires_at: OffsetDateTime,
}

pub enum Rejected {
    Invalid,
    Expired,
}

/// Looks a token up, checks its expiry and that its owner is still allowed, and records the use.
pub async fn check(st: &AppState, token: &str) -> sqlx::Result<Result<Caller, Rejected>> {
    let row: Option<(Uuid, Uuid, String, OffsetDateTime, Option<String>)> = sqlx::query_as(
        "SELECT t.id, t.user_id, t.name, t.expires_at, u.owner_claim FROM api_tokens t \
         JOIN users u ON u.id = t.user_id WHERE t.hash = $1",
    )
    .bind(hash(token))
    .fetch_optional(&st.db)
    .await?;
    let Some((id, user_id, name, expires_at, claim)) = row else { return Ok(Err(Rejected::Invalid)) };
    if !claim.is_some_and(|c| st.cfg.owners.contains(&c)) {
        return Ok(Err(Rejected::Invalid));
    }
    let now = OffsetDateTime::now_utc();
    if expires_at <= now {
        return Ok(Err(Rejected::Expired));
    }
    sqlx::query("UPDATE api_tokens SET last_used_at = $2 WHERE id = $1").bind(id).bind(now).execute(&st.db).await?;
    Ok(Ok(Caller { user_id, name, expires_at }))
}

// ---------- tokens page ----------

struct Row {
    id: Uuid,
    name: String,
    prefix: String,
    created: (String, String),
    expires: (String, String),
    last_used: Option<(String, String)>,
    expired: bool,
}

#[derive(Template)]
#[template(path = "admin_tokens.html")]
struct TokensPage<'a> {
    l: Layout<'a>,
    rows: Vec<Row>,
    /// The token just created, shown this once.
    new_token: Option<String>,
    error: Option<String>,
    /// The owner signed in before the server stored owner claims.
    needs_sign_in: bool,
}

async fn render(st: &AppState, owner: Uuid, new_token: Option<String>, error: Option<&str>) -> Response {
    let l = layout(st, owner, "tokens", "/tokens").await;
    let data = async {
        let rows: Vec<(Uuid, String, String, OffsetDateTime, OffsetDateTime, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT id, name, prefix, created_at, expires_at, last_used_at FROM api_tokens \
                 WHERE user_id = $1 ORDER BY created_at DESC",
        )
        .bind(owner)
        .fetch_all(&st.db)
        .await?;
        let claim: Option<String> =
            sqlx::query_scalar("SELECT owner_claim FROM users WHERE id = $1").bind(owner).fetch_one(&st.db).await?;
        Ok::<_, sqlx::Error>((rows, claim.is_none()))
    };
    let (rows, needs_sign_in) = match data.await {
        Ok(d) => d,
        Err(e) => return internal(e),
    };
    let now = OffsetDateTime::now_utc();
    let time = |t| (rfc3339(t), utc_text(t));
    let rows = rows
        .into_iter()
        .map(|(id, name, prefix, created, expires, used)| Row {
            id,
            name,
            prefix,
            created: time(created),
            expires: time(expires),
            last_used: used.map(time),
            expired: expires <= now,
        })
        .collect();
    let error = error.map(|k| l.t.s(k).to_string());
    let mut res = html(TokensPage { l, rows, new_token, error, needs_sign_in });
    // The page can hold a new token.
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

pub async fn page(State(st): State<AppState>, Extension(OwnerId(owner)): Extension<OwnerId>) -> Response {
    render(&st, owner, None, None).await
}

#[derive(Deserialize)]
pub struct CreateForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    days: i64,
}

/// Renders the page with the new token instead of redirecting, so the token never sits in the session.
pub async fn create_post(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Form(f): Form<CreateForm>,
) -> Response {
    let name = f.name.trim();
    if name.is_empty() {
        return render(&st, owner, None, Some("tokens.name_required")).await;
    }
    let days = if EXPIRY_DAYS.contains(&f.days) { f.days } else { 90 };
    let has_claim: Option<String> =
        match sqlx::query_scalar("SELECT owner_claim FROM users WHERE id = $1").bind(owner).fetch_one(&st.db).await {
            Ok(c) => c,
            Err(e) => return internal(e),
        };
    // The page already shows the sign-in-again notice.
    if has_claim.is_none() {
        return render(&st, owner, None, None).await;
    }
    match create(&st.db, owner, name, days).await {
        Ok(token) => render(&st, owner, Some(token), None).await,
        Err(e) => internal(e),
    }
}

pub async fn revoke(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
) -> Response {
    match sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(owner)
        .execute(&st.db)
        .await
    {
        Ok(_) => Redirect::to("/tokens").into_response(),
        Err(e) => internal(e),
    }
}

/// Drops the session, so the next visit to the tokens page signs in again and stores the owner claim.
pub async fn sign_in_again(session: Session) -> Response {
    if let Err(e) = session.flush().await {
        return internal(e);
    }
    Redirect::to("/tokens").into_response()
}
