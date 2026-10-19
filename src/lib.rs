pub mod admin;
pub mod api;
pub mod auth;
pub mod claim;
pub mod cli;
pub mod config;
pub mod i18n;
pub mod icons;
pub mod ignore_rules;
pub mod lifecycle;
pub mod paths;
pub mod render;
pub mod share;
pub mod store;
pub mod token;
pub mod tokens;
pub mod upload;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use include_dir::{Dir, include_dir};
use object_store::{ObjectStoreExt, PutPayload};
use tokio_util::task::TaskTracker;
use tower::ServiceExt;
use tower_sessions::{SessionManagerLayer, cookie::SameSite};
use tower_sessions_sqlx_store::PostgresStore;
use uuid::Uuid;

pub use config::Config;

pub type AppState = Arc<Inner>;

pub struct Inner {
    pub cfg: Config,
    pub db: sqlx::PgPool,
    pub store: store::Store,
    pub sessions: PostgresStore,
    pub oidc: auth::Oidc,
    pub i18n: i18n::I18n,
    pub renderer: render::Renderer,
    pub syntax_css: String,
    /// Upload tasks. Shutdown waits for them.
    pub tracker: TaskTracker,
    /// Wrong password attempts per client IP and share.
    pub attempts: Mutex<HashMap<(IpAddr, Uuid), Vec<Instant>>>,
}

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

impl Inner {
    pub async fn new(cfg: Config, db: sqlx::PgPool, store: store::Store) -> anyhow::Result<AppState> {
        MIGRATOR.run(&db).await?;
        let sessions = PostgresStore::new(db.clone());
        sessions.migrate().await?;
        Ok(Arc::new(Inner {
            cfg,
            db,
            store,
            sessions,
            oidc: auth::Oidc::new(),
            i18n: i18n::I18n::load(),
            renderer: render::Renderer::default(),
            syntax_css: render::syntax_css(),
            tracker: TaskTracker::new(),
            attempts: Mutex::default(),
        }))
    }
}

pub static ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/static");

/// Serves `/_dropgo/assets/*` from the files embedded at build time.
pub async fn asset(State(st): State<AppState>, axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    let body: Body = if path == "syntax.css" {
        st.syntax_css.clone().into()
    } else if let Some(f) = ASSETS.get_file(&path) {
        f.contents().into()
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ct = mime_guess::from_path(&path).first_or_octet_stream();
    (
        [
            (header::CONTENT_TYPE, ct.as_ref().to_string()),
            (header::CACHE_CONTROL, "public, max-age=3600".into()),
            // Sandboxed pages fetch fonts in CORS mode with an opaque origin.
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".into()),
        ],
        body,
    )
        .into_response()
}

async fn healthz(State(st): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&st.db).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// One router that picks the admin or share app by the `Host` header.
pub fn router(st: AppState) -> Router {
    let admin = admin::router(st.clone()).layer(
        SessionManagerLayer::new(st.sessions.clone())
            .with_name("__Host-dropgo-admin")
            .with_secure(true)
            .with_http_only(true)
            .with_same_site(SameSite::Lax)
            .with_path("/"),
    );
    let share = share::router(st.clone()).layer(
        SessionManagerLayer::new(st.sessions.clone())
            .with_name("__Host-dropgo-share")
            .with_secure(true)
            .with_http_only(true)
            .with_same_site(SameSite::None)
            .with_path("/"),
    );
    let st2 = st.clone();
    Router::new()
        .route("/_dropgo/healthz", get(healthz))
        .fallback(move |req: Request| {
            let (admin, share, st) = (admin.clone(), share.clone(), st2.clone());
            async move {
                let host = req
                    .headers()
                    .get(header::HOST)
                    .and_then(|h| h.to_str().ok())
                    .or_else(|| req.uri().authority().map(|a| a.as_str()))
                    .unwrap_or_default();
                let mut res = if host.eq_ignore_ascii_case(st.cfg.admin_host()) {
                    admin.oneshot(req).await.into_response()
                } else if host.eq_ignore_ascii_case(st.cfg.share_host()) {
                    share.oneshot(req).await.into_response()
                } else {
                    StatusCode::NOT_FOUND.into_response()
                };
                res.headers_mut().insert("x-robots-tag", HeaderValue::from_static("noindex, nofollow, noarchive"));
                res
            }
        })
        .with_state(st)
}

/// Waits for upload tasks after the server stops taking requests.
pub async fn drain(st: &AppState) {
    st.tracker.close();
    st.tracker.wait().await;
}

/// Rebuilds the stored pages of every `.md` file, after a renderer or stylesheet change.
pub async fn rerender(st: &AppState) -> anyhow::Result<()> {
    let shares: Vec<(uuid::Uuid, uuid::Uuid)> =
        sqlx::query_as("SELECT id, upload_id FROM shares WHERE state <> 'purging'").fetch_all(&st.db).await?;
    let mut pages = 0;
    for (id, upload) in shares {
        for (rel, _) in store::list_files(&st.store, id, upload).await? {
            if !rel.ends_with(".md") {
                continue;
            }
            let md = store::get_bytes(&st.store, &store::file_key(id, upload, &rel)).await?;
            let name = rel.rsplit('/').next().unwrap_or(&rel);
            let page = st.renderer.page(&String::from_utf8_lossy(&md), name);
            st.store.put(&store::rendered_key(id, upload, &rel), PutPayload::from(page)).await?;
            pages += 1;
        }
    }
    tracing::info!("rerendered {pages} pages");
    Ok(())
}
