//! Test harness: one Postgres container per test binary, a fresh database per test,
//! the in-memory store, and requests sent straight into the router.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

use axum::{Router, body::Body, http::Request};
use dropgo::{AppState, Config, Inner, store::Store};
use http_body_util::BodyExt;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{Container, ImageExt, runners::SyncRunner},
};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use tower_sessions::{
    SessionStore,
    session::{Id, Record},
};
use uuid::Uuid;

pub const ADMIN: &str = "https://drop.test";
pub const SHARE: &str = "https://s.test";

/// shortcut: statics never drop, so each test binary leaves its container running. Clean up with
/// `docker rm -f $(docker ps -q --filter label=org.testcontainers.managed-by=testcontainers)`.
static PG: OnceLock<(Container<Postgres>, String)> = OnceLock::new();

/// The container is started on another thread: the sync runner brings its own runtime.
fn pg_url() -> &'static str {
    &PG.get_or_init(|| {
        std::thread::spawn(|| {
            let c = Postgres::default().with_tag("17-alpine").start().expect("start postgres");
            let port = c.get_host_port_ipv4(5432).expect("port");
            let url = format!("postgres://postgres:postgres@127.0.0.1:{port}");
            (c, url)
        })
        .join()
        .unwrap()
    })
    .1
}

pub fn config() -> Config {
    Config {
        listen: "127.0.0.1:0".into(),
        admin_origin: ADMIN.into(),
        share_origin: SHARE.into(),
        oidc_issuer: "http://127.0.0.1:1/default".into(),
        oidc_client_id: "dropgo".into(),
        oidc_client_secret: "secret".into(),
        owners: vec!["alice".into()],
        owner_claim: "preferred_username".into(),
        secret: "test-secret-test-secret-test-secret".into(),
        database_url: String::new(),
        store_url: "memory://".into(),
        max_share_bytes: 1024 * 1024,
        tmp_dir: None,
        trusted_proxies: vec![],
    }
}

pub struct App {
    pub st: AppState,
    pub router: Router,
}

pub struct Res {
    pub status: u16,
    pub headers: axum::http::HeaderMap,
    pub body: bytes::Bytes,
}

impl Res {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn header(&self, name: &str) -> &str {
        self.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default()
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or_default()
    }
    /// `name=value` of the first Set-Cookie that sets `name`.
    pub fn cookie(&self, name: &str) -> Option<String> {
        self.headers
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|c| c.starts_with(&format!("{name}=")))
            .map(|c| c.split(';').next().unwrap().to_string())
    }
}

impl App {
    pub async fn new() -> App {
        App::with(config(), dropgo::store::from_url("memory://").unwrap()).await
    }

    pub async fn with(cfg: Config, store: Store) -> App {
        let base = pg_url();
        let name = format!("t_{}", Uuid::new_v4().simple());
        let admin = sqlx::PgPool::connect(&format!("{base}/postgres")).await.unwrap();
        sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
        admin.close().await;
        let db =
            sqlx::postgres::PgPoolOptions::new().max_connections(20).connect(&format!("{base}/{name}")).await.unwrap();
        let st = Inner::new(cfg, db, store).await.unwrap();
        let router = dropgo::router(st.clone());
        App { st, router }
    }

    /// Creates a user and returns its id and a session cookie value for `__Host-dropgo-*`.
    /// The subject doubles as the stored owner claim.
    pub async fn user(&self, subject: &str) -> (Uuid, String) {
        let id = dropgo::auth::find_or_create_user(&self.st.db, "https://idp.test", subject, "en").await.unwrap();
        sqlx::query("UPDATE users SET owner_claim = $2 WHERE id = $1")
            .bind(id)
            .bind(subject)
            .execute(&self.st.db)
            .await
            .unwrap();
        let until = OffsetDateTime::now_utc() + Duration::hours(12);
        let mut rec = Record {
            id: Id::default(),
            data: HashMap::from([(
                "owner".to_string(),
                serde_json::json!({ "user_id": id, "until": until.unix_timestamp() }),
            )]),
            expiry_date: until,
        };
        self.st.sessions.create(&mut rec).await.unwrap();
        (id, rec.id.to_string())
    }

    /// A user with a session and an API token.
    pub async fn token(&self, subject: &str) -> (Uuid, String, String) {
        let (id, session) = self.user(subject).await;
        let token = dropgo::tokens::create(&self.st.db, id, "test", 90).await.unwrap();
        (id, session, token)
    }

    /// An `/api/v1` request with a bearer token and an optional JSON body, without an Origin header.
    pub async fn api(&self, method: &str, path: &str, token: &str, json: Option<serde_json::Value>) -> Res {
        let auth = format!("Bearer {token}");
        let mut h = vec![("authorization", auth.as_str())];
        let body = match json {
            Some(v) => {
                h.push(("content-type", "application/json"));
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        self.send(req(method, &format!("{ADMIN}/api/v1{path}"), &h, body)).await
    }

    /// Uploads through the API with settings first, as the CLI sends them.
    pub async fn api_upload(&self, token: &str, settings: &[(&str, &str)], files: &[(&str, &[u8])]) -> Res {
        let (ct, body) = multipart(settings, files);
        let auth = format!("Bearer {token}");
        self.send(req(
            "POST",
            &format!("{ADMIN}/api/v1/shares"),
            &[("authorization", &auth), ("content-type", &ct)],
            Body::from(body),
        ))
        .await
    }

    pub async fn send(&self, req: Request<Body>) -> Res {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        Res { status: parts.status.as_u16(), headers: parts.headers, body: body.collect().await.unwrap().to_bytes() }
    }

    pub async fn get(&self, url: &str, headers: &[(&str, &str)]) -> Res {
        self.send(req("GET", url, headers, Body::empty())).await
    }

    /// A page navigation, as a browser sends it.
    pub async fn nav(&self, url: &str, cookies: &str) -> Res {
        self.get(url, &[("sec-fetch-mode", "navigate"), ("cookie", cookies)]).await
    }

    pub async fn post_form(&self, url: &str, cookies: &str, form: &[(&str, &str)]) -> Res {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let origin = if url.starts_with(ADMIN) { ADMIN } else { SHARE };
        self.send(req(
            "POST",
            url,
            &[("cookie", cookies), ("origin", origin), ("content-type", "application/x-www-form-urlencoded")],
            Body::from(body),
        ))
        .await
    }

    /// Posts an upload as the admin page does: settings first, then `f:<path>` parts or a `zip` part.
    pub async fn upload(&self, session: &str, settings: &[(&str, &str)], files: &[(&str, &[u8])]) -> Res {
        let (ct, body) = multipart(settings, files);
        self.send(req(
            "POST",
            &format!("{ADMIN}/api/upload"),
            &[("cookie", &format!("__Host-dropgo-admin={session}")), ("origin", ADMIN), ("content-type", &ct)],
            Body::from(body),
        ))
        .await
    }

    /// Uploads files at `path` with the given visibility and returns the share id.
    pub async fn share(&self, session: &str, path: &str, visibility: &str, files: &[(&str, &[u8])]) -> Uuid {
        let mut settings = vec![("path", path), ("visibility", visibility), ("expiry", "never")];
        if visibility == "password" {
            settings.push(("password", "hunter2"));
        }
        let res = self.upload(session, &settings, files).await;
        assert_eq!(res.status, 200, "{}", res.text());
        self.share_id(path).await
    }

    pub async fn share_id(&self, path: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM shares WHERE path = $1 AND state = 'active'")
            .bind(path)
            .fetch_one(&self.st.db)
            .await
            .unwrap()
    }

    /// Every object key in the store.
    pub async fn keys(&self) -> Vec<String> {
        use futures::TryStreamExt;
        let mut keys: Vec<String> =
            self.st.store.list(None).map_ok(|m| m.location.to_string()).try_collect().await.unwrap();
        keys.sort();
        keys
    }
}

pub fn req(method: &str, url: &str, headers: &[(&str, &str)], body: Body) -> Request<Body> {
    let u = url::Url::parse(url).unwrap();
    let mut path = u.path().to_string();
    if let Some(q) = u.query() {
        path = format!("{path}?{q}");
    }
    let mut b = Request::builder().method(method).uri(path).header("host", u.host_str().unwrap());
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(body).unwrap()
}

pub fn multipart(settings: &[(&str, &str)], files: &[(&str, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "----dropgo-test-boundary";
    let mut body = Vec::new();
    for (k, v) in settings {
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    for (name, data) in files {
        let filename = name.rsplit('/').next().unwrap();
        body.extend(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend(*data);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

pub fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

pub fn admin_cookie(session: &str) -> String {
    format!("__Host-dropgo-admin={session}")
}

pub fn share_cookie(session: &str) -> String {
    format!("__Host-dropgo-share={session}")
}
