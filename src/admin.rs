//! The admin host: upload page, share list, edit, trash, preferences.

use askama::Template;
use axum::{
    Extension, Form, Router,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339, macros::format_description};
use uuid::Uuid;

use crate::{
    AppState, asset,
    auth::{self, OwnerId},
    claim::{self, ClaimError},
    i18n::{LOCALES, T},
    lifecycle::{self, RestoreError, TRASH_DAYS},
    paths,
    share::ShareRow,
    tokens,
    upload::{self, Fail},
};

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; \
                   form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

pub fn router(st: AppState) -> Router {
    // Room for multipart headers on top of the file bytes, which `upload` counts itself.
    let body_limit = st.cfg.max_share_bytes as usize + 16 * 1024 * 1024;
    let owner_only = Router::new()
        .route("/", get(upload_page))
        .route("/shares", get(list))
        .route("/shares/{id}/edit", get(edit_page).post(edit))
        .route("/shares/{id}/delete", post(delete))
        .route("/shares/{id}/restore", post(restore))
        .route("/shares/{id}/purge", post(purge))
        .route("/prefs", post(prefs))
        .route("/tokens", get(tokens::page).post(tokens::create_post))
        .route("/tokens/{id}/revoke", post(tokens::revoke))
        .route("/api/check", get(upload::check_path))
        .route("/api/upload", post(upload::upload).layer(DefaultBodyLimit::max(body_limit)))
        .route_layer(middleware::from_fn_with_state(st.clone(), auth::require_owner));
    Router::new()
        .merge(owner_only)
        .route("/auth/callback", get(auth::admin_callback))
        .route("/_dropgo/assets/{*path}", get(asset))
        .layer(middleware::from_fn_with_state(st.clone(), auth::check_origin))
        .layer(middleware::from_fn(headers))
        .with_state(st.clone())
        // Outside the layers above: bearer only, so no Origin check, and file responses keep their sandbox CSP.
        .nest("/api/v1", crate::api::router(st))
}

async fn headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("private, no-cache"));
    res
}

/// What the base layout needs on every page.
pub(crate) struct Layout<'a> {
    pub(crate) t: T<'a>,
    pub(crate) theme: String,
    pub(crate) nav: &'static str,
    pub(crate) back: String,
}

impl Layout<'_> {
    pub(crate) fn other_theme(&self) -> &'static str {
        if self.theme == "dark" { "light" } else { "dark" }
    }
    pub(crate) fn other_locale(&self) -> &'static str {
        LOCALES.into_iter().find(|l| *l != self.t.locale).unwrap_or("en")
    }
}

pub(crate) async fn layout<'a>(st: &'a AppState, owner: Uuid, nav: &'static str, back: &str) -> Layout<'a> {
    let (locale, theme): (String, String) = sqlx::query_as("SELECT locale, theme FROM users WHERE id = $1")
        .bind(owner)
        .fetch_optional(&st.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| ("en".into(), "dark".into()));
    Layout { t: st.i18n.t(&locale), theme, nav, back: back.to_string() }
}

pub(crate) fn html(page: impl Template) -> Response {
    match page.render() {
        Ok(s) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], s).into_response(),
        Err(e) => internal(e),
    }
}

pub(crate) fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!("admin: {e}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

pub(crate) fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Shown until admin.js swaps in the browser's local time.
pub(crate) fn utc_text(t: OffsetDateTime) -> String {
    let f = format_description!("[year]-[month]-[day] [hour]:[minute] UTC");
    t.format(&f).unwrap_or_default()
}

fn human_size(b: i64) -> String {
    match b {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} B"),
    }
}

/// Whole days left before the job purges a share, rounded up.
pub fn days_left(trashed_at: OffsetDateTime, now: OffsetDateTime) -> i64 {
    let secs = (trashed_at + time::Duration::days(TRASH_DAYS) - now).whole_seconds();
    (secs.max(0) + 86_399) / 86_400
}

// ---------- upload ----------

#[derive(Template)]
#[template(path = "admin_upload.html")]
struct UploadPage<'a> {
    l: Layout<'a>,
    share_origin: &'a str,
    max_bytes: u64,
    max_size: String,
    /// Strings for upload.mjs, as JSON in a non-executed script tag.
    strings: String,
}

const JS_KEYS: [&str; 15] = [
    "upload.js.files",
    "upload.js.skipped",
    "upload.js.no_entry",
    "upload.js.entry",
    "upload.js.too_big",
    "upload.js.too_many",
    "upload.js.uploading",
    "upload.js.failed",
    "upload.js.copied",
    "upload.js.overwrite",
    "upload.js.bad_path",
    "upload.js.zip",
    "upload.js.network",
    "form.password_keep",
    "form.password_hint",
];

async fn upload_page(State(st): State<AppState>, Extension(OwnerId(owner)): Extension<OwnerId>) -> Response {
    let l = layout(&st, owner, "upload", "/").await;
    let strings: serde_json::Map<String, serde_json::Value> =
        JS_KEYS.iter().map(|k| (k.to_string(), l.t.s(k).into())).collect();
    // `</` cannot appear inside the script tag.
    let strings = serde_json::to_string(&strings).unwrap_or_default().replace("</", "<\\/");
    html(UploadPage {
        l,
        share_origin: &st.cfg.share_origin,
        max_bytes: st.cfg.max_share_bytes,
        max_size: human_size(st.cfg.max_share_bytes as i64),
        strings,
    })
}

// ---------- list and trash ----------

struct Row {
    id: Uuid,
    url: String,
    path: String,
    visibility: String,
    /// `public`, `password` or `owner`, for the icon.
    vis: String,
    /// The entry file and its type, shown so the search can match them.
    entry: String,
    kind: &'static str,
    expires: Option<(String, String)>,
    size: String,
    created: (String, String),
    days_left: i64,
    purging: bool,
    /// Restore needs a new path for this one.
    ask_path: bool,
}

#[derive(Template)]
#[template(path = "admin_list.html")]
struct ListPage<'a> {
    l: Layout<'a>,
    trash: bool,
    rows: Vec<Row>,
    error: Option<String>,
}

#[derive(Deserialize, Default)]
struct ListQuery {
    #[serde(default)]
    tab: String,
}

async fn list(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Query(q): Query<ListQuery>,
) -> Response {
    render_list(&st, owner, q, None, None).await
}

/// The owner's active shares, or the trash, newest first. The API also filters by path.
pub(crate) async fn list_shares(
    st: &AppState,
    owner: Uuid,
    trash: bool,
    path: Option<&str>,
) -> sqlx::Result<Vec<ShareRow>> {
    sqlx::query_as(
        "SELECT * FROM shares WHERE owner_id = $1 AND ($2 AND state <> 'active' OR NOT $2 AND state = 'active') \
         AND ($3::text IS NULL OR path = $3) ORDER BY created_at DESC",
    )
    .bind(owner)
    .bind(trash)
    .bind(path)
    .fetch_all(&st.db)
    .await
}

async fn render_list(
    st: &AppState,
    owner: Uuid,
    q: ListQuery,
    error: Option<String>,
    ask_path: Option<Uuid>,
) -> Response {
    let trash = q.tab == "trash";
    let back = if trash { "/shares?tab=trash" } else { "/shares" };
    let l = layout(st, owner, "shares", back).await;
    let rows = match list_shares(st, owner, trash, None).await {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let now = OffsetDateTime::now_utc();
    let rows = rows
        .into_iter()
        .map(|s| Row {
            id: s.id,
            url: format!("{}/{}/", st.cfg.share_origin, s.path),
            visibility: l.t.s(&format!("visibility.{}", s.visibility)).to_string(),
            vis: s.visibility.clone(),
            kind: if s.entry.ends_with(".md") { "Markdown" } else { "HTML" },
            entry: s.entry.rsplit('/').next().unwrap_or_default().to_string(),
            expires: s.expires_at.map(|e| (rfc3339(e), utc_text(e))),
            size: human_size(s.size_bytes),
            created: (rfc3339(s.created_at), utc_text(s.created_at)),
            days_left: s.trashed_at.map_or(0, |t| days_left(t, now)),
            purging: s.state == "purging",
            ask_path: ask_path == Some(s.id),
            path: s.path,
        })
        .collect();
    html(ListPage { l, trash, rows, error })
}

async fn delete(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
) -> Response {
    match lifecycle::delete(&st, owner, id, OffsetDateTime::now_utc()).await {
        Ok(_) => Redirect::to("/shares").into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct RestoreForm {
    #[serde(default)]
    path: String,
}

async fn restore(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
    Form(f): Form<RestoreForm>,
) -> Response {
    let t = upload::owner_t(&st, owner).await;
    let trash = || ListQuery { tab: "trash".into() };
    let new_path = match paths::parse_share_path(&f.path) {
        Ok(p) => p,
        Err(e) => return render_list(&st, owner, trash(), Some(upload::path_fail(e).message(t)), Some(id)).await,
    };
    match lifecycle::restore(&st, owner, id, new_path.as_deref(), OffsetDateTime::now_utc()).await {
        Ok(Ok(())) => Redirect::to("/shares").into_response(),
        Ok(Err(RestoreError::NotFound)) => {
            render_list(&st, owner, trash(), Some(t.s("restore.not_found").into()), None).await
        }
        Ok(Err(RestoreError::Conflict(p))) => {
            let msg = t.s("restore.conflict").replace("{}", &p);
            render_list(&st, owner, trash(), Some(msg), Some(id)).await
        }
        Err(e) => internal(e),
    }
}

async fn purge(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
) -> Response {
    match lifecycle::purge(&st, owner, id).await {
        Ok(_) => Redirect::to("/shares?tab=trash").into_response(),
        // The share stays in `purging`; the job retries it.
        Err(e) => internal(e),
    }
}

// ---------- edit ----------

#[derive(Template)]
#[template(path = "admin_edit.html")]
struct EditPage<'a> {
    l: Layout<'a>,
    share: ShareRow,
    path: String,
    visibility: String,
    expires: Option<(String, String)>,
    error: Option<String>,
}

pub(crate) async fn active_share(st: &AppState, owner: Uuid, id: Uuid) -> sqlx::Result<Option<ShareRow>> {
    sqlx::query_as("SELECT * FROM shares WHERE id = $1 AND owner_id = $2 AND state = 'active'")
        .bind(id)
        .bind(owner)
        .fetch_optional(&st.db)
        .await
}

async fn edit_page(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
) -> Response {
    match active_share(&st, owner, id).await {
        Ok(Some(share)) => {
            let l = layout(&st, owner, "shares", &format!("/shares/{id}/edit")).await;
            let page = EditPage {
                l,
                path: share.path.clone(),
                visibility: share.visibility.clone(),
                expires: share.expires_at.map(|e| (rfc3339(e), utc_text(e))),
                share,
                error: None,
            };
            html(page)
        }
        Ok(None) => Redirect::to("/shares").into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
pub(crate) struct EditForm {
    pub(crate) path: String,
    pub(crate) visibility: String,
    #[serde(default)]
    pub(crate) password: String,
    pub(crate) clear_password: Option<String>,
    /// `keep`, empty for never, or a time.
    #[serde(default)]
    pub(crate) expires_at: String,
}

async fn edit(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
    Form(f): Form<EditForm>,
) -> Response {
    let share = match active_share(&st, owner, id).await {
        Ok(Some(s)) => s,
        Ok(None) => return Redirect::to("/shares").into_response(),
        Err(e) => return internal(e),
    };
    match save_edit(&st, owner, &share, &f).await {
        Ok(()) => Redirect::to("/shares").into_response(),
        Err(fail) => {
            let l = layout(&st, owner, "shares", &format!("/shares/{id}/edit")).await;
            let error = Some(fail.message(l.t));
            let expires = share.expires_at.map(|e| (rfc3339(e), utc_text(e)));
            html(EditPage { l, share, path: f.path, visibility: f.visibility, expires, error })
        }
    }
}

pub(crate) async fn save_edit(st: &AppState, owner: Uuid, share: &ShareRow, f: &EditForm) -> Result<(), Fail> {
    let now = OffsetDateTime::now_utc();
    let path = paths::parse_share_path(&f.path)
        .map_err(upload::path_fail)?
        .ok_or_else(|| Fail::new(StatusCode::BAD_REQUEST, "path.required"))?;
    let visibility = match upload::parse_visibility(&f.visibility)? {
        // The form offers public and owner; a new or kept password makes a public share a password share.
        "public" if !f.password.is_empty() || share.password_hash.is_some() && f.clear_password.is_none() => "password",
        v => v,
    };
    let expires_at = match f.expires_at.as_str() {
        "keep" => share.expires_at,
        "" => None,
        custom => upload::parse_expiry("custom", custom, now)?,
    };
    let hash = match f.password.as_str() {
        p if !p.is_empty() && visibility == "password" => Some(crate::share::hash_password(p.to_string()).await),
        _ => None,
    };
    let mut tx = st.db.begin().await.map_err(upload::internal)?;
    match claim::claim(&mut tx, owner, &path, Some(share.id), None, now).await.map_err(upload::internal)? {
        Ok(_) => {}
        Err(ClaimError::Conflict(p)) => return Err(Fail::new(StatusCode::CONFLICT, "path.conflict").with_arg(p)),
        Err(ClaimError::NeedsOverwrite(_)) => {
            return Err(Fail::new(StatusCode::CONFLICT, "path.conflict").with_arg(path));
        }
    }
    let ok: Option<bool> = sqlx::query_scalar(
        "UPDATE shares SET path = $3, visibility = $4, \
         password_hash = CASE WHEN $4 = 'password' THEN COALESCE($5, password_hash) END, expires_at = $6 \
         WHERE id = $1 AND owner_id = $2 AND state = 'active' \
         RETURNING password_hash IS NOT NULL OR visibility <> 'password'",
    )
    .bind(share.id)
    .bind(owner)
    .bind(&path)
    .bind(visibility)
    .bind(&hash)
    .bind(expires_at)
    .fetch_optional(&mut *tx)
    .await
    .map_err(upload::internal)?;
    match ok {
        Some(true) => tx.commit().await.map_err(upload::internal),
        Some(false) => Err(Fail::new(StatusCode::BAD_REQUEST, "form.password_required")),
        None => Err(Fail::new(StatusCode::NOT_FOUND, "restore.not_found")),
    }
}

// ---------- preferences ----------

#[derive(Deserialize)]
struct PrefsForm {
    theme: Option<String>,
    locale: Option<String>,
    #[serde(default)]
    back: String,
}

async fn prefs(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Form(f): Form<PrefsForm>,
) -> Response {
    let theme = f.theme.filter(|t| t == "dark" || t == "light");
    let locale = f.locale.filter(|l| LOCALES.contains(&l.as_str()));
    if let Err(e) =
        sqlx::query("UPDATE users SET theme = COALESCE($2, theme), locale = COALESCE($3, locale) WHERE id = $1")
            .bind(owner)
            .bind(theme)
            .bind(locale)
            .execute(&st.db)
            .await
    {
        return internal(e);
    }
    Redirect::to(&auth::safe_return(&f.back)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_days_left() {
        let now = OffsetDateTime::now_utc();
        assert_eq!(days_left(now - time::Duration::days(10), now), 20);
        assert_eq!(days_left(now - time::Duration::days(10) - time::Duration::minutes(1), now), 20);
        assert_eq!(days_left(now, now), 30);
        assert_eq!(days_left(now - time::Duration::days(31), now), 0);
    }
}
