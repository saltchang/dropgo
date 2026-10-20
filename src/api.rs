//! `/api/v1` on the admin host: JSON for the CLI and coding agents. Bearer tokens only, never the session.

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use object_store::ObjectStoreExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AppState, admin,
    auth::OwnerId,
    lifecycle::{self, RestoreError, TRASH_DAYS},
    paths,
    share::{self, ShareRow},
    store,
    tokens::{self, Caller, Rejected},
    upload::{self, CheckQuery, Fail},
};

pub fn router(st: AppState) -> Router {
    // Same room as `/api/upload`: the upload counts the file bytes itself.
    let body_limit = st.cfg.max_share_bytes as usize + 16 * 1024 * 1024;
    Router::new()
        .route("/me", get(me))
        .route("/check", get(check))
        .route("/shares", get(list).merge(post(create).layer(DefaultBodyLimit::max(body_limit))))
        .route("/shares/{id}", get(get_one).patch(edit))
        .route("/shares/{id}/trash", post(trash))
        .route("/shares/{id}/restore", post(restore))
        .route("/shares/{id}/files", get(files))
        .route("/shares/{id}/files/{*rel}", get(file))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(middleware::from_fn_with_state(st.clone(), bearer))
        .layer(middleware::from_fn_with_state(st.clone(), json_errors))
        .with_state(st)
}

/// A failure that `json_errors` renders, since rendering needs the catalog.
struct ApiFail(Fail);

impl From<Fail> for ApiFail {
    fn from(f: Fail) -> Self {
        ApiFail(f)
    }
}

impl IntoResponse for ApiFail {
    fn into_response(self) -> Response {
        let mut res = self.0.status.into_response();
        res.extensions_mut().insert(self.0);
        res
    }
}

fn fail(status: StatusCode, key: &'static str) -> ApiFail {
    ApiFail(Fail::new(status, key))
}

fn internal(e: impl std::fmt::Display) -> ApiFail {
    ApiFail(upload::internal(e))
}

/// Gives every error the `{"error": {code, message, details}}` shape, including axum's own rejections.
async fn json_errors(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let f = match res.extensions_mut().remove::<Fail>() {
        Some(f) => f,
        None if res.status().is_client_error() || res.status().is_server_error() => {
            let key = match res.status() {
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => "api.not_found",
                StatusCode::PAYLOAD_TOO_LARGE => "upload.too_big",
                s if s.is_server_error() => "upload.internal",
                _ => "api.bad_request",
            };
            Fail::new(res.status(), key)
        }
        None => return res,
    };
    let mut details = serde_json::Map::new();
    if let Some(a) = &f.arg {
        details.insert("arg".into(), a.clone().into());
    }
    if let Some(e) = &f.existing {
        details.insert("existing".into(), serde_json::to_value(e).unwrap_or_default());
    }
    let message = f.message(st.i18n.t("en"));
    (f.status, Json(json!({ "error": { "code": f.key, "message": message, "details": details } }))).into_response()
}

async fn bearer(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, t)| t.trim().to_string());
    let Some(token) = token else { return fail(StatusCode::UNAUTHORIZED, "auth.missing_token").into_response() };
    match tokens::check(&st, &token).await {
        Ok(Ok(caller)) => {
            req.extensions_mut().insert(OwnerId(caller.user_id));
            req.extensions_mut().insert(caller);
            next.run(req).await
        }
        Ok(Err(Rejected::Invalid)) => fail(StatusCode::UNAUTHORIZED, "auth.invalid_token").into_response(),
        Ok(Err(Rejected::Expired)) => fail(StatusCode::UNAUTHORIZED, "auth.token_expired").into_response(),
        Err(e) => internal(e).into_response(),
    }
}

#[derive(Serialize)]
struct ShareJson {
    id: Uuid,
    path: String,
    url: String,
    state: String,
    visibility: String,
    entry: String,
    file_count: i32,
    size_bytes: i64,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    expires_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    trashed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    purge_at: Option<OffsetDateTime>,
}

fn share_json(st: &AppState, s: ShareRow) -> ShareJson {
    ShareJson {
        id: s.id,
        url: format!("{}/{}/", st.cfg.share_origin, s.path),
        path: s.path,
        state: s.state,
        visibility: s.visibility,
        entry: s.entry,
        file_count: s.file_count,
        size_bytes: s.size_bytes,
        created_at: s.created_at,
        expires_at: s.expires_at,
        purge_at: s.trashed_at.map(|t| t + time::Duration::days(TRASH_DAYS)),
        trashed_at: s.trashed_at,
    }
}

/// The owner's share in any state.
async fn find(st: &AppState, owner: Uuid, id: Uuid) -> Result<ShareRow, ApiFail> {
    sqlx::query_as("SELECT * FROM shares WHERE id = $1 AND owner_id = $2")
        .bind(id)
        .bind(owner)
        .fetch_optional(&st.db)
        .await
        .map_err(internal)?
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, "share.not_found"))
}

async fn find_active(st: &AppState, owner: Uuid, id: Uuid) -> Result<ShareRow, ApiFail> {
    let s = find(st, owner, id).await?;
    if s.state == "active" { Ok(s) } else { Err(fail(StatusCode::NOT_FOUND, "share.not_found")) }
}

async fn reply(st: &AppState, owner: Uuid, id: Uuid) -> Result<Response, ApiFail> {
    Ok(Json(share_json(st, find(st, owner, id).await?)).into_response())
}

async fn me(Extension(c): Extension<Caller>) -> Response {
    Json(json!({ "name": c.name, "expires_at": admin::rfc3339(c.expires_at) })).into_response()
}

async fn check(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Query(q): Query<CheckQuery>,
) -> Result<Response, ApiFail> {
    let path = upload::check(&st, owner, &q.path).await?;
    Ok(Json(json!({ "ok": true, "path": path })).into_response())
}

#[derive(Deserialize)]
struct ListQuery {
    state: Option<String>,
    path: Option<String>,
}

async fn list(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiFail> {
    let trash = match q.state.as_deref() {
        None | Some("active") => false,
        Some("trash") => true,
        Some(_) => return Err(fail(StatusCode::BAD_REQUEST, "api.bad_request")),
    };
    let path = paths::parse_share_path(q.path.as_deref().unwrap_or_default()).map_err(upload::path_fail)?;
    let rows = admin::list_shares(&st, owner, trash, path.as_deref()).await.map_err(internal)?;
    Ok(Json(rows.into_iter().map(|s| share_json(&st, s)).collect::<Vec<_>>()).into_response())
}

async fn get_one(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiFail> {
    reply(&st, owner, id).await
}

async fn create(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    mp: Multipart,
) -> Result<Response, ApiFail> {
    let (id, _) = upload::receive_and_store(&st, owner, mp).await?;
    Ok((StatusCode::CREATED, reply(&st, owner, id).await?).into_response())
}

/// Applies only the fields present. Unlike the Web UI form, a password never turns a public share into a
/// password share: what the agent sends is what it gets.
async fn edit(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
    Json(m): Json<serde_json::Map<String, Value>>,
) -> Result<Response, ApiFail> {
    let bad = || fail(StatusCode::BAD_REQUEST, "api.bad_request");
    let text = |k: &str| match m.get(k) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(bad()),
    };
    let share = find_active(&st, owner, id).await?;
    let visibility = upload::parse_visibility(&text("visibility")?.unwrap_or_else(|| share.visibility.clone()))?;
    let password = text("password")?.unwrap_or_default();
    if !password.is_empty() && visibility != "password" {
        return Err(fail(StatusCode::BAD_REQUEST, "form.password_unused"));
    }
    let expires_at = match m.get("expires_at") {
        None => "keep".to_string(),
        Some(Value::Null) => String::new(),
        // `save_edit` reads "" as never and "keep" as unchanged; here both are just bad times.
        Some(Value::String(s)) if s.is_empty() || s == "keep" => {
            return Err(fail(StatusCode::BAD_REQUEST, "form.bad_expiry"));
        }
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err(bad()),
    };
    let form = admin::EditForm {
        path: text("path")?.unwrap_or_else(|| share.path.clone()),
        visibility: visibility.to_string(),
        password,
        clear_password: (visibility != "password").then(|| "1".to_string()),
        expires_at,
    };
    admin::save_edit(&st, owner, &share, &form).await?;
    reply(&st, owner, id).await
}

/// Idempotent, so an agent can retry after a network error.
async fn trash(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiFail> {
    if find(&st, owner, id).await?.state == "active" {
        lifecycle::delete(&st, owner, id, OffsetDateTime::now_utc()).await.map_err(internal)?;
    }
    reply(&st, owner, id).await
}

#[derive(Deserialize)]
struct RestoreBody {
    path: Option<String>,
}

async fn restore(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
    body: Option<Json<RestoreBody>>,
) -> Result<Response, ApiFail> {
    // `lifecycle::restore` cannot tell a missing share from one that is not in the trash.
    if find(&st, owner, id).await?.state != "trashed" {
        return Err(fail(StatusCode::CONFLICT, "share.not_trashed"));
    }
    let new_path = match body.and_then(|Json(b)| b.path) {
        Some(p) => paths::parse_share_path(&p).map_err(upload::path_fail)?,
        None => None,
    };
    match lifecycle::restore(&st, owner, id, new_path.as_deref(), OffsetDateTime::now_utc()).await {
        Ok(Ok(())) => reply(&st, owner, id).await,
        Ok(Err(RestoreError::NotFound)) => Err(fail(StatusCode::NOT_FOUND, "share.not_found")),
        Ok(Err(RestoreError::Conflict(p))) => {
            Err(Fail::new(StatusCode::CONFLICT, "restore.conflict").with_arg(p).into())
        }
        Err(e) => Err(internal(e)),
    }
}

async fn files(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiFail> {
    let s = find_active(&st, owner, id).await?;
    let files = store::list_files(&st.store, s.id, s.upload_id).await.map_err(internal)?;
    let files: Vec<Value> = files.into_iter().map(|(path, size)| json!({ "path": path, "size": size })).collect();
    Ok(Json(files).into_response())
}

#[derive(Deserialize)]
struct FileQuery {
    rendered: Option<String>,
}

/// Uploaded bytes never run on the admin origin: sandboxed, not sniffed, and downloaded rather than shown.
async fn file(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Path((id, rel)): Path<(Uuid, String)>,
    Query(q): Query<FileQuery>,
) -> Result<Response, ApiFail> {
    let s = find_active(&st, owner, id).await?;
    let missing = || fail(StatusCode::NOT_FOUND, "file.not_found");
    let rendered = q.rendered.as_deref().is_some_and(|v| v == "1" || v == "true");
    if !paths::valid_rel_path(&rel) || rendered && !rel.ends_with(".md") {
        return Err(missing());
    }
    let (key, ct) = if rendered {
        (store::rendered_key(s.id, s.upload_id, &rel), "text/html; charset=utf-8".to_string())
    } else {
        (store::file_key(s.id, s.upload_id, &rel), share::content_type(&rel))
    };
    let body = match st.store.get(&key).await {
        Ok(r) => Body::from_stream(r.into_stream()),
        Err(object_store::Error::NotFound { .. }) => return Err(missing()),
        Err(e) => return Err(internal(e)),
    };
    let mut res = ([(header::CONTENT_TYPE, ct)], body).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(share::SANDBOX));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment"));
    Ok(res)
}
