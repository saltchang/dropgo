//! The share host: serving shares, access checks, the password form, downloads, theme toggle.

use std::io::Write as _;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use askama::Template;
use axum::{
    Extension, Form, Router,
    body::Body,
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{any, get, post},
};
use object_store::ObjectStoreExt;
use serde::Deserialize;
use time::OffsetDateTime;
use tower_sessions::Session;
use uuid::Uuid;

use crate::{
    AppState, asset,
    auth::{self, Host},
    i18n::{self, T},
    icons, paths, render, store,
    token::{self, Kind},
};

pub const THEME_COOKIE: &str = "__Host-dropgo-theme";
pub(crate) const SANDBOX: &str = "sandbox allow-scripts allow-forms allow-popups allow-modals allow-downloads";
const MAX_WRONG_PER_MINUTE: usize = 5;

#[derive(sqlx::FromRow, Clone, Debug)]
pub struct ShareRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub path: String,
    pub state: String,
    pub visibility: String,
    pub password_hash: Option<String>,
    pub upload_id: Uuid,
    pub entry: String,
    pub file_count: i32,
    pub size_bytes: i64,
    pub created_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
    pub trashed_at: Option<OffsetDateTime>,
}

impl ShareRow {
    pub fn live(&self) -> bool {
        self.state == "active" && self.expires_at.is_none_or(|e| e > OffsetDateTime::now_utc())
    }
    fn credential(&self) -> String {
        token::credential(&self.visibility, self.password_hash.as_deref())
    }
}

pub fn router(st: AppState) -> Router {
    Router::new()
        .route("/_dropgo/assets/{*path}", get(asset))
        .route("/_dropgo/auth/callback", get(auth::share_callback))
        .route("/_dropgo/theme", get(theme))
        .route("/_dropgo/unlock/{id}", post(unlock))
        .route("/_dropgo/download/{id}", get(download))
        .route("/_dropgo/t/{token}/{*rest}", get(serve_token))
        .route("/_dropgo", any(not_found))
        .route("/_dropgo/{*rest}", any(not_found))
        .fallback(serve_plain)
        .layer(middleware::from_fn(base_headers))
        .with_state(st)
}

async fn base_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("private, no-cache"));
    res
}

/// Per-request bits every page needs.
struct Ctx<'a> {
    st: &'a AppState,
    session: Session,
    headers: HeaderMap,
    owner: Option<Uuid>,
}

impl<'a> Ctx<'a> {
    async fn new(st: &'a AppState, session: Session, headers: HeaderMap) -> Ctx<'a> {
        let owner = auth::current_owner(st, &session).await;
        Ctx { st, session, headers, owner }
    }

    fn t(&self) -> T<'a> {
        self.st.i18n.t(i18n::negotiate(self.headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok())))
    }

    /// Theme cookie, else the signed-in owner's saved theme, else dark.
    async fn theme(&self) -> &'static str {
        match cookie(&self.headers, THEME_COOKIE).as_deref() {
            Some("light") => return "light",
            Some("dark") => return "dark",
            _ => {}
        }
        if let Some(id) = self.owner {
            let saved: Option<String> = sqlx::query_scalar("SELECT theme FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.st.db)
                .await
                .ok()
                .flatten();
            if saved.as_deref() == Some("light") {
                return "light";
            }
        }
        "dark"
    }

    fn is_navigation(&self) -> bool {
        match self.headers.get("sec-fetch-mode") {
            Some(m) => m == "navigate",
            None => {
                self.headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("text/html"))
            }
        }
    }
}

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(name)?.strip_prefix('=').map(str::to_string))
}

// ---------- dropgo's own pages ----------

#[derive(Template)]
#[template(path = "share_message.html")]
struct MessagePage<'a> {
    t: T<'a>,
    theme: &'a str,
    toggle: String,
    title: &'a str,
    message: &'a str,
}

#[derive(Template)]
#[template(path = "share_password.html")]
struct PasswordPage<'a> {
    t: T<'a>,
    theme: &'a str,
    toggle: String,
    share_id: Uuid,
    back: &'a str,
    error: Option<&'a str>,
}

fn own_page(st: &AppState, status: StatusCode, html: String) -> Response {
    let csp = format!("default-src 'none'; style-src {}/_dropgo/assets/; form-action 'self'", st.cfg.share_origin);
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()), (header::CONTENT_SECURITY_POLICY, csp)],
        html,
    )
        .into_response()
}

fn toggle_href(theme: &str, back: &str) -> String {
    let other = if theme == "dark" { "light" } else { "dark" };
    let back: String = url::form_urlencoded::byte_serialize(back.as_bytes()).collect();
    format!("/_dropgo/theme?set={other}&back={back}")
}

async fn message(ctx: &Ctx<'_>, status: StatusCode, key: &str, back: &str) -> Response {
    let t = ctx.t();
    let theme = ctx.theme().await;
    let page = MessagePage {
        t,
        theme,
        toggle: toggle_href(theme, back),
        title: t.s(&format!("share.{key}.title")),
        message: t.s(&format!("share.{key}.body")),
    };
    own_page(ctx.st, status, page.render().unwrap_or_default())
}

async fn password_form(ctx: &Ctx<'_>, status: StatusCode, share_id: Uuid, back: &str, error: Option<&str>) -> Response {
    let theme = ctx.theme().await;
    let page = PasswordPage { t: ctx.t(), theme, toggle: toggle_href(theme, back), share_id, back, error };
    own_page(ctx.st, status, page.render().unwrap_or_default())
}

async fn not_found(State(st): State<AppState>, session: Session, headers: HeaderMap, uri: Uri) -> Response {
    let ctx = Ctx::new(&st, session, headers).await;
    message(&ctx, StatusCode::NOT_FOUND, "not_found", uri.path()).await
}

// ---------- lookup and access ----------

enum Found {
    Live(ShareRow),
    Gone,
    Missing,
}

async fn lookup(st: &AppState, decoded_path: &str) -> sqlx::Result<Found> {
    let candidates = paths::lookup_candidates(decoded_path);
    let row: Option<ShareRow> = sqlx::query_as(
        "SELECT * FROM shares WHERE path = ANY($1) ORDER BY (state = 'active') DESC, length(path) DESC LIMIT 1",
    )
    .bind(&candidates)
    .fetch_optional(&st.db)
    .await?;
    Ok(match row {
        Some(s) if s.live() => Found::Live(s),
        Some(_) => Found::Gone,
        None => Found::Missing,
    })
}

/// `None` means access is granted. `back` is the URL to return to after sign-in or unlock.
async fn deny(ctx: &Ctx<'_>, share: &ShareRow, back: &str) -> Option<Response> {
    if share.visibility == "public" || ctx.owner == Some(share.owner_id) {
        return None;
    }
    let nav = ctx.is_navigation();
    match share.visibility.as_str() {
        "owner" if ctx.owner.is_some() => Some(message(ctx, StatusCode::NOT_FOUND, "not_found", back).await),
        "owner" if nav => Some(auth::start_login(ctx.st, &ctx.session, Host::Share, back).await),
        "password" => {
            let unlocked = cookie(&ctx.headers, &format!("__Host-dropgo-unlock-{}", share.id)).is_some_and(|c| {
                token::verify(&ctx.st.cfg.secret, Kind::Unlock, share.id, &share.credential(), &c, now())
            });
            if unlocked {
                None
            } else if nav {
                Some(password_form(ctx, StatusCode::UNAUTHORIZED, share.id, back, None).await)
            } else {
                Some(StatusCode::UNAUTHORIZED.into_response())
            }
        }
        _ => Some(StatusCode::UNAUTHORIZED.into_response()),
    }
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

// ---------- serving ----------

async fn serve_plain(
    State(st): State<AppState>,
    session: Session,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let ctx = Ctx::new(&st, session, headers).await;
    serve(&ctx, &uri, None).await
}

async fn serve_token(
    State(st): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path((tok, _)): Path<(String, String)>,
    uri: Uri,
) -> Response {
    let ctx = Ctx::new(&st, session, headers).await;
    serve(&ctx, &uri, Some(tok)).await
}

async fn serve(ctx: &Ctx<'_>, uri: &Uri, tok: Option<String>) -> Response {
    let st = ctx.st;
    let prefix = tok.as_ref().map(|t| format!("/_dropgo/t/{t}")).unwrap_or_default();
    // The request path without the token prefix, still percent-encoded.
    let raw = &uri.path()[prefix.len()..];
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let back = format!("{raw}{query}");
    let Ok(decoded) = percent_encoding::percent_decode_str(raw).decode_utf8() else {
        return message(ctx, StatusCode::NOT_FOUND, "not_found", &back).await;
    };
    if decoded.contains('\0') {
        return message(ctx, StatusCode::NOT_FOUND, "not_found", &back).await;
    }
    let share = match lookup(st, &decoded).await {
        Ok(Found::Live(s)) => s,
        Ok(Found::Gone) => return message(ctx, StatusCode::GONE, "gone", &back).await,
        Ok(Found::Missing) => return message(ctx, StatusCode::NOT_FOUND, "not_found", &back).await,
        Err(e) => return internal(e),
    };

    let nav = ctx.is_navigation();
    match &tok {
        Some(t) if token::verify(&st.cfg.secret, Kind::Path, share.id, &share.credential(), t, now()) => {}
        Some(_) if nav => return Redirect::to(&auth::safe_return(&back)).into_response(),
        Some(_) => return StatusCode::UNAUTHORIZED.into_response(),
        None => {
            if let Some(res) = deny(ctx, &share, &back).await {
                return res;
            }
            if share.visibility != "public" && nav {
                let t = token::sign(&st.cfg.secret, Kind::Path, share.id, &share.credential(), now());
                return Redirect::to(&format!("/_dropgo/t/{t}{back}")).into_response();
            }
        }
    }

    // Split the decoded path into the share part and the file part, keeping the file's case.
    let k = share.path.split('/').count();
    let segs: Vec<&str> = decoded.trim_start_matches('/').split('/').collect();
    if segs.len() == k {
        let enc_share = raw.trim_start_matches('/');
        return moved(format!("{prefix}/{enc_share}/{query}"));
    }
    let rest = segs[k..].join("/");
    serve_file(ctx, &share, &rest, &format!("{prefix}{back}")).await
}

async fn serve_file(ctx: &Ctx<'_>, share: &ShareRow, rest: &str, current_url: &str) -> Response {
    let st = ctx.st;
    let target = if rest.is_empty() {
        share.entry.clone()
    } else if let Some(dir) = rest.strip_suffix('/') {
        match dir_index(st, share, dir).await {
            Some(t) => t,
            None => return message(ctx, StatusCode::NOT_FOUND, "not_found", current_url).await,
        }
    } else {
        rest.to_string()
    };
    if !paths::valid_rel_path(&target) {
        return message(ctx, StatusCode::NOT_FOUND, "not_found", current_url).await;
    }

    if target.ends_with(".md") {
        let key = store::rendered_key(share.id, share.upload_id, &target);
        let stored = match store::get_bytes(&st.store, &key).await {
            Ok(b) => b,
            Err(object_store::Error::NotFound { .. }) => {
                return message(ctx, StatusCode::NOT_FOUND, "not_found", current_url).await;
            }
            Err(e) => return internal(e),
        };
        let theme = ctx.theme().await;
        let html = String::from_utf8_lossy(&stored);
        let footer = footer(ctx.t(), share, theme, current_url);
        let page = render::personalize(&html, theme, &footer).unwrap_or_else(|| html.into_owned());
        return content(([(header::CONTENT_TYPE, "text/html; charset=utf-8")], page).into_response());
    }

    let key = store::file_key(share.id, share.upload_id, &target);
    match st.store.get(&key).await {
        Ok(r) => {
            let ct = content_type(&target);
            content(([(header::CONTENT_TYPE, ct)], Body::from_stream(r.into_stream())).into_response())
        }
        Err(object_store::Error::NotFound { .. }) => {
            // A directory without the trailing slash.
            if dir_index(st, share, &target).await.is_some() {
                let (path, query) = current_url.split_once('?').map_or((current_url, ""), |(p, q)| (p, q));
                let query = if query.is_empty() { String::new() } else { format!("?{query}") };
                // `//notes/sub` must not become a scheme-relative `//notes/sub/`.
                return moved(format!("/{}/{query}", path.trim_start_matches('/')));
            }
            message(ctx, StatusCode::NOT_FOUND, "not_found", current_url).await
        }
        Err(e) => internal(e),
    }
}

async fn dir_index(st: &AppState, share: &ShareRow, dir: &str) -> Option<String> {
    for name in ["index.html", "index.md"] {
        let rel = format!("{dir}/{name}");
        if st.store.head(&store::file_key(share.id, share.upload_id, &rel)).await.is_ok() {
            return Some(rel);
        }
    }
    None
}

fn moved(to: String) -> Response {
    (StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, to)]).into_response()
}

pub(crate) fn content_type(name: &str) -> String {
    let m = mime_guess::from_path(name).first_or_octet_stream();
    if m.type_() == mime_guess::mime::TEXT || m.subtype() == mime_guess::mime::JAVASCRIPT {
        format!("{m}; charset=utf-8")
    } else {
        m.to_string()
    }
}

/// Headers for anything that serves uploaded or rendered content.
fn content(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(SANDBOX));
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    res
}

fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!("{e}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

fn footer(t: T<'_>, share: &ShareRow, theme: &str, current_url: &str) -> String {
    let dl = |fmt: &str, key: &str| {
        format!(r#"<a href="/_dropgo/download/{}?format={fmt}">{}{}</a>"#, share.id, icons::icon("download"), t.s(key))
    };
    let mut links = vec![dl("zip", "footer.zip")];
    if share.file_count == 1 {
        links.push(dl("source", "footer.source"));
    }
    if share.entry.ends_with(".md") {
        links.push(dl("html", "footer.html"));
    }
    let toggle_key = if theme == "dark" { "theme.to_light" } else { "theme.to_dark" };
    links.push(format!(
        r#"<a class="dropgo-theme" href="{}">{}{}</a>"#,
        render::escape(&toggle_href(theme, current_url)),
        icons::icon(if theme == "dark" { "sun" } else { "moon" }),
        t.s(toggle_key)
    ));
    format!(
        r#"<footer class="dropgo-footer"><nav><span class="dropgo-mark">{}dropgo</span>{}</nav></footer>"#,
        icons::icon("download"),
        links.join(" ")
    )
}

// ---------- password unlock ----------

#[derive(Deserialize)]
struct UnlockForm {
    password: String,
    back: String,
}

pub fn client_ip(cfg: &crate::Config, headers: &HeaderMap, peer: Option<IpAddr>) -> IpAddr {
    let peer = peer.unwrap_or(IpAddr::from([0, 0, 0, 0]));
    let trusted = |ip: &IpAddr| cfg.trusted_proxies.iter().any(|c| c.contains(*ip));
    if !trusted(&peer) {
        return peer;
    }
    let hops: Vec<IpAddr> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|h| h.trim().parse().ok())
        .collect();
    hops.into_iter().rev().find(|ip| !trusted(ip)).unwrap_or(peer)
}

/// The address guesses are counted under. One IPv6 subscriber gets a whole /64, so it counts as one client.
fn attempt_ip(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => IpAddr::V6((u128::from(v6) & !((1u128 << 64) - 1)).into()),
        v4 => v4,
    }
}

/// Counts guesses in the last minute and records this one, so concurrent guesses share the limit.
/// Returns false when the limit is reached. `shortcut:` in memory, so only right with one replica.
fn take_attempt(st: &AppState, key: (IpAddr, Uuid)) -> bool {
    let mut map = st.attempts.lock().expect("attempts lock");
    let now = Instant::now();
    map.retain(|_, v| {
        v.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        !v.is_empty()
    });
    let seen = map.entry(key).or_default();
    if seen.len() >= MAX_WRONG_PER_MINUTE {
        return false;
    }
    seen.push(now);
    true
}

/// A right password gives its attempt back.
fn return_attempt(st: &AppState, key: (IpAddr, Uuid)) {
    if let Some(v) = st.attempts.lock().expect("attempts lock").get_mut(&key) {
        v.pop();
    }
}

async fn unlock(
    State(st): State<AppState>,
    session: Session,
    headers: HeaderMap,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    Path(id): Path<Uuid>,
    Form(form): Form<UnlockForm>,
) -> Response {
    let ctx = Ctx::new(&st, session, headers).await;
    let back = auth::safe_return(&form.back);
    let share: Option<ShareRow> =
        match sqlx::query_as("SELECT * FROM shares WHERE id = $1").bind(id).fetch_optional(&st.db).await {
            Ok(s) => s,
            Err(e) => return internal(e),
        };
    let Some(share) = share.filter(|s| s.live() && s.visibility == "password") else {
        return message(&ctx, StatusCode::NOT_FOUND, "not_found", &back).await;
    };
    let key = (attempt_ip(client_ip(&st.cfg, &ctx.headers, peer.map(|p| p.0.0.ip()))), share.id);
    if !take_attempt(&st, key) {
        return message(&ctx, StatusCode::TOO_MANY_REQUESTS, "too_many", &back).await;
    }
    let hash = share.password_hash.clone().unwrap_or_default();
    if !verify_password(form.password, hash).await {
        let t = ctx.t();
        let err = t.s("password.wrong");
        return password_form(&ctx, StatusCode::UNAUTHORIZED, share.id, &back, Some(err)).await;
    }
    return_attempt(&st, key);
    let cred = share.credential();
    let unlock = token::sign(&st.cfg.secret, Kind::Unlock, share.id, &cred, now());
    let set = format!(
        "__Host-dropgo-unlock-{}={unlock}; Max-Age={}; Path=/; Secure; HttpOnly; SameSite=None",
        share.id,
        token::UNLOCK_SECS
    );
    // Downloads are top-level navigations and carry the cookie; share pages need the path token.
    let to = if back.starts_with("/_dropgo/") {
        back
    } else {
        format!("/_dropgo/t/{}{back}", token::sign(&st.cfg.secret, Kind::Path, share.id, &cred, now()))
    };
    (StatusCode::SEE_OTHER, [(header::SET_COOKIE, set), (header::LOCATION, to)]).into_response()
}

pub async fn hash_password(pw: String) -> String {
    use argon2::password_hash::PasswordHasher;
    tokio::task::spawn_blocking(move || {
        argon2::Argon2::default().hash_password(pw.as_bytes()).expect("argon2").to_string()
    })
    .await
    .expect("hash task")
}

async fn verify_password(pw: String, hash: String) -> bool {
    use argon2::password_hash::{PasswordVerifier, phc::PasswordHash};
    // Each check takes about 19 MiB, so a burst of guesses must not run them all at once.
    static ARGON2: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let Ok(permit) = ARGON2.acquire().await else { return false };
    tokio::task::spawn_blocking(move || {
        // Held until argon2 finishes, even if the client has gone.
        let _permit = permit;
        PasswordHash::new(&hash).is_ok_and(|h| argon2::Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
    })
    .await
    .unwrap_or(false)
}

// ---------- theme ----------

#[derive(Deserialize)]
struct ThemeQuery {
    set: String,
    #[serde(default)]
    back: String,
}

/// Writes only the browser cookie, never the owner's saved theme: any site can trigger this GET.
async fn theme(Query(q): Query<ThemeQuery>) -> Response {
    let theme = if q.set == "light" { "light" } else { "dark" };
    let set = format!("{THEME_COOKIE}={theme}; Max-Age=31536000; Path=/; Secure; HttpOnly; SameSite=Lax");
    (StatusCode::SEE_OTHER, [(header::SET_COOKIE, set), (header::LOCATION, auth::safe_return(&q.back))]).into_response()
}

// ---------- downloads ----------

#[derive(Deserialize)]
struct DownloadQuery {
    format: String,
}

async fn download(
    State(st): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
    uri: Uri,
) -> Response {
    let ctx = Ctx::new(&st, session, headers).await;
    let back = uri.path_and_query().map_or("/", |p| p.as_str()).to_string();
    let share: Option<ShareRow> =
        match sqlx::query_as("SELECT * FROM shares WHERE id = $1").bind(id).fetch_optional(&st.db).await {
            Ok(s) => s,
            Err(e) => return internal(e),
        };
    let share = match share {
        Some(s) if s.live() => s,
        Some(_) => return message(&ctx, StatusCode::GONE, "gone", &back).await,
        None => return message(&ctx, StatusCode::NOT_FOUND, "not_found", &back).await,
    };
    if let Some(res) = deny(&ctx, &share, &back).await {
        return res;
    }
    let base = share.path.rsplit('/').next().unwrap_or("share").to_string();
    let entry_name = share.entry.rsplit('/').next().unwrap_or(&share.entry).to_string();
    let result = match q.format.as_str() {
        "zip" => {
            zip_share(&st, &share).await.map(|b| (format!("{base}.zip"), "application/zip".to_string(), Body::from(b)))
        }
        "source" if share.file_count == 1 => {
            let key = store::file_key(share.id, share.upload_id, &share.entry);
            st.store
                .get(&key)
                .await
                .map(|r| (entry_name.clone(), content_type(&entry_name), Body::from_stream(r.into_stream())))
                .map_err(Into::into)
        }
        "html" if share.entry.ends_with(".md") => {
            let key = store::file_key(share.id, share.upload_id, &share.entry);
            store::get_bytes(&st.store, &key).await.map_err(Into::into).map(|b| {
                let css = crate::ASSETS.get_file("dropgo.css").and_then(|f| f.contents_utf8()).unwrap_or_default();
                let html = st.renderer.standalone(&String::from_utf8_lossy(&b), &entry_name, css);
                let name = format!("{}.html", entry_name.trim_end_matches(".md"));
                (name, "text/html; charset=utf-8".to_string(), Body::from(html))
            })
        }
        _ => return message(&ctx, StatusCode::NOT_FOUND, "not_found", &back).await,
    };
    match result {
        Ok((name, ct, body)) => {
            let disp = format!(
                "attachment; filename=\"{}\"; filename*=UTF-8''{}",
                name.replace(['"', '\\'], "_"),
                percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC)
            );
            content(([(header::CONTENT_TYPE, ct), (header::CONTENT_DISPOSITION, disp)], body).into_response())
        }
        Err(e) => internal(e),
    }
}

/// `shortcut:` builds the zip in memory, bounded by the share size limit and two at a time,
/// so anyone with a public link can use at most about 4x that limit. Stream it if limits grow.
async fn zip_share(st: &AppState, share: &ShareRow) -> anyhow::Result<Vec<u8>> {
    static ZIPS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = ZIPS.acquire().await?;
    let mut files = Vec::new();
    for (rel, _) in store::list_files(&st.store, share.id, share.upload_id).await? {
        let bytes = store::get_bytes(&st.store, &store::file_key(share.id, share.upload_id, &rel)).await?;
        files.push((rel, bytes));
    }
    // Deflate is CPU work; keep it off the async threads.
    tokio::task::spawn_blocking(move || {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (rel, bytes) in files {
            zip.start_file(rel.as_str(), opts)?;
            zip.write_all(&bytes)?;
        }
        Ok(zip.finish()?.into_inner())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_ip_skips_only_trusted_hops() {
        let cfg = crate::Config {
            listen: String::new(),
            admin_origin: String::new(),
            share_origin: String::new(),
            oidc_issuer: String::new(),
            oidc_client_id: String::new(),
            oidc_client_secret: String::new(),
            owners: vec![],
            owner_claim: String::new(),
            secret: String::new(),
            database_url: String::new(),
            store_url: String::new(),
            max_share_bytes: 0,
            tmp_dir: None,
            trusted_proxies: vec![crate::config::parse_cidr("10.0.0.0/8").unwrap()],
        };
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "6.6.6.6, 1.2.3.4, 10.0.0.2".parse().unwrap());
        assert_eq!(
            client_ip(&cfg, &h, Some(ip("10.0.0.1"))),
            ip("1.2.3.4"),
            "right-most untrusted hop, not the spoofed left one"
        );
        assert_eq!(client_ip(&cfg, &h, Some(ip("5.5.5.5"))), ip("5.5.5.5"), "an untrusted peer is the client");
        h.insert("x-forwarded-for", "10.0.0.3".parse().unwrap());
        assert_eq!(client_ip(&cfg, &h, Some(ip("10.0.0.1"))), ip("10.0.0.1"), "all hops trusted");
    }

    #[test]
    fn attempts_count_per_ipv6_64() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert_eq!(attempt_ip(ip("2001:db8:1:2:aaaa::1")), attempt_ip(ip("2001:db8:1:2:bbbb::9")));
        assert_ne!(attempt_ip(ip("2001:db8:1:2::1")), attempt_ip(ip("2001:db8:1:3::1")));
        assert_eq!(attempt_ip(ip("::ffff:1.2.3.4")), ip("1.2.3.4"));
        assert_ne!(attempt_ip(ip("1.2.3.4")), attempt_ip(ip("1.2.3.5")));
    }

    #[test]
    fn cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; __Host-dropgo-theme=light; b=2".parse().unwrap());
        assert_eq!(cookie(&h, THEME_COOKIE).as_deref(), Some("light"));
        assert_eq!(cookie(&h, "__Host-dropgo"), None);
    }

    #[test]
    fn toggle_link() {
        assert_eq!(toggle_href("dark", "/a b/?x=1"), "/_dropgo/theme?set=light&back=%2Fa+b%2F%3Fx%3D1");
    }
}
