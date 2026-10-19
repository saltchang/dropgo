//! owner-auth spec, against navikt/mock-oauth2-server.
mod common;
use std::sync::OnceLock;

use common::*;
use openidconnect::reqwest;
use testcontainers_modules::testcontainers::{
    Container, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::SyncRunner,
};

static IDP: OnceLock<(Container<GenericImage>, String)> = OnceLock::new();

fn issuer() -> &'static str {
    &IDP.get_or_init(|| {
        std::thread::spawn(|| {
            let c = GenericImage::new("ghcr.io/navikt/mock-oauth2-server", "2.1.10")
                .with_exposed_port(8080.tcp())
                .with_wait_for(WaitFor::Nothing)
                .with_env_var("JSON_CONFIG", r#"{"interactiveLogin":true}"#)
                .start()
                .expect("start mock oidc");
            let port = c.get_host_port_ipv4(8080).unwrap();
            (c, format!("http://localhost:{port}/default"))
        })
        .join()
        .unwrap()
    })
    .1
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

async fn app_with(owners: &[&str]) -> App {
    app_at(issuer(), owners, "preferred_username").await
}

/// An issuer in front of the mock provider whose userinfo endpoint answers `userinfo`. The mock takes its
/// issuer from the Host header, so tokens name the proxy and pass verification.
async fn proxied_issuer(userinfo: serde_json::Value) -> String {
    use axum::{body::Bytes, extract::Request, http::StatusCode, response::IntoResponse};
    let upstream = issuer().trim_end_matches("/default").to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new()
        .route("/default/userinfo", axum::routing::get(move || async move { axum::Json(userinfo) }))
        .fallback(move |req: Request| {
            let upstream = upstream.clone();
            async move {
                let (parts, body) = req.into_parts();
                let body: Bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
                let res = http()
                    .request(parts.method, format!("{upstream}{}", parts.uri))
                    .headers(parts.headers)
                    .body(body)
                    .send()
                    .await
                    .unwrap();
                let status = StatusCode::from_u16(res.status().as_u16()).unwrap();
                let headers = res.headers().clone();
                (status, headers, res.bytes().await.unwrap()).into_response()
            }
        });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/default")
}

async fn app_at(iss: &str, owners: &[&str], owner_claim: &str) -> App {
    // The mock server needs a few seconds to start.
    for _ in 0..60 {
        if http().get(format!("{iss}/.well-known/openid-configuration")).send().await.is_ok_and(|r| r.status() == 200) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let mut cfg = config();
    cfg.oidc_issuer = iss.to_string();
    cfg.owners = owners.iter().map(|o| o.to_string()).collect();
    cfg.owner_claim = owner_claim.into();
    App::with(cfg, dropgo::store::from_url("memory://").unwrap()).await
}

struct Login {
    /// The response that started sign-in.
    start: Res,
    /// The callback response.
    done: Res,
    cookie_name: &'static str,
}

impl Login {
    fn session(&self) -> String {
        let c = self.done.cookie(self.cookie_name).or_else(|| self.start.cookie(self.cookie_name)).unwrap();
        c.split_once('=').unwrap().1.to_string()
    }
}

/// Opens `url` signed out, logs in at the provider as `subject` with `username`, and follows the callback.
async fn sign_in(app: &App, url: &str, subject: &str, username: &str) -> Login {
    let cookie_name = if url.starts_with(ADMIN) { "__Host-dropgo-admin" } else { "__Host-dropgo-share" };
    let start = app.get(url, &[("sec-fetch-mode", "navigate"), ("accept-language", "zh-TW,zh;q=0.9")]).await;
    assert_eq!(start.status, 302, "{}", start.text());
    let authorize = start.header("location").to_string();
    let claims = serde_json::json!({ "preferred_username": username }).to_string();
    let res = http().post(&authorize).form(&[("username", subject), ("claims", claims.as_str())]).send().await.unwrap();
    assert_eq!(res.status(), 302);
    let callback = res.headers()["location"].to_str().unwrap().to_string();
    let session = start.cookie(cookie_name).unwrap();
    let done = app.get(&callback, &[("cookie", &session), ("accept-language", "zh-TW,zh;q=0.9")]).await;
    Login { start, done, cookie_name }
}

#[tokio::test]
async fn signed_out_visit_redirects_with_pkce() {
    let app = app_with(&["alice"]).await;
    let res = app.get(&format!("{ADMIN}/"), &[]).await;
    assert_eq!(res.status, 302);
    let loc = res.header("location");
    assert!(loc.starts_with(&format!("{}/authorize", issuer())));
    assert!(loc.contains("code_challenge=") && loc.contains("code_challenge_method=S256"));
    let expiry: time::OffsetDateTime =
        sqlx::query_scalar("SELECT expiry_date FROM tower_sessions.session").fetch_one(&app.st.db).await.unwrap();
    assert!(expiry < time::OffsetDateTime::now_utc() + time::Duration::hours(1), "an unfinished sign-in expires soon");
}

#[tokio::test]
async fn allowed_user_lands_on_first_page() {
    let app = app_with(&["alice"]).await;
    let login = sign_in(&app, &format!("{ADMIN}/shares?q=x"), "sub-1", "alice").await;
    assert_eq!((login.done.status, login.done.header("location")), (303, "/shares?q=x"));
    // The session id changes at sign-in.
    assert_ne!(login.done.cookie(login.cookie_name), login.start.cookie(login.cookie_name));
    let page = app.get(&format!("{ADMIN}/"), &[("cookie", &admin_cookie(&login.session()))]).await;
    assert_eq!(page.status, 200);
    let counts: (i64, i64, String, Option<String>) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM identities), (SELECT locale FROM users), \
         (SELECT owner_claim FROM users)",
    )
    .fetch_one(&app.st.db)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, "zh-TW".into(), Some("alice".into())), "locale from Accept-Language, claim stored");
}

#[tokio::test]
async fn user_not_on_allowlist() {
    let app = app_with(&["alice"]).await;
    let login = sign_in(&app, &format!("{ADMIN}/"), "sub-2", "mallory").await;
    assert_eq!(login.done.status, 403);
    assert_eq!(app.get(&format!("{ADMIN}/"), &[("cookie", &admin_cookie(&login.session()))]).await.status, 302);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn session_ends_after_twelve_hours() {
    let app = app_with(&["alice"]).await;
    let id = dropgo::auth::find_or_create_user(&app.st.db, "x", "y", "en").await.unwrap();
    let past = time::OffsetDateTime::now_utc() - time::Duration::seconds(1);
    let mut rec = tower_sessions::session::Record {
        id: Default::default(),
        data: [("owner".to_string(), serde_json::json!({ "user_id": id, "until": past.unix_timestamp() }))].into(),
        expiry_date: past + time::Duration::hours(1),
    };
    tower_sessions::SessionStore::create(&app.st.sessions, &mut rec).await.unwrap();
    assert_eq!(app.get(&format!("{ADMIN}/"), &[("cookie", &admin_cookie(&rec.id.to_string()))]).await.status, 302);
}

// A foreign return URL is covered by the `safe_return` unit test: such paths 404 before sign-in starts.

#[tokio::test]
async fn username_change_keeps_the_user() {
    let app = app_with(&["alice", "alice2"]).await;
    sign_in(&app, &format!("{ADMIN}/"), "sub-4", "alice").await;
    let again = sign_in(&app, &format!("{ADMIN}/"), "sub-4", "alice2").await;
    assert_eq!(again.done.status, 303);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn owner_only_share_signs_in_on_share_host() {
    let app = app_with(&["alice"]).await;
    let first = sign_in(&app, &format!("{ADMIN}/"), "sub-5", "alice").await;
    app.share(&first.session(), "mine", "owner", &[("f:index.html", b"<p>mine")]).await;

    let login = sign_in(&app, &format!("{SHARE}/mine/"), "sub-5", "alice").await;
    assert_eq!((login.done.status, login.done.header("location")), (303, "/mine/"));
    let set = login.start.headers.get("set-cookie").unwrap().to_str().unwrap().to_string();
    for attr in ["__Host-dropgo-share=", "Secure", "HttpOnly", "SameSite=None", "Path=/"] {
        assert!(set.contains(attr), "{set}");
    }
    let page = app.nav(&format!("{SHARE}/mine/"), &share_cookie(&login.session())).await;
    assert_eq!(page.status, 303, "to a token URL");
    assert_eq!(app.nav(&format!("{SHARE}{}", page.header("location")), "").await.text(), "<p>mine");
}

#[tokio::test]
async fn owner_claim_from_userinfo() {
    // `owner` is not in the mock's ID token, so it can only come from userinfo.
    let app_for =
        |info: serde_json::Value| async move { app_at(&proxied_issuer(info).await, &["alice"], "owner").await };

    let app = app_for(serde_json::json!({ "sub": "sub-6", "owner": "alice" })).await;
    assert_eq!(sign_in(&app, &format!("{ADMIN}/"), "sub-6", "x").await.done.status, 303);

    let app = app_for(serde_json::json!({ "sub": "sub-6", "owner": "mallory" })).await;
    assert_eq!(sign_in(&app, &format!("{ADMIN}/"), "sub-6", "x").await.done.status, 403);

    let app = app_for(serde_json::json!({ "sub": "someone-else", "owner": "alice" })).await;
    assert_eq!(
        sign_in(&app, &format!("{ADMIN}/"), "sub-6", "x").await.done.status,
        400,
        "userinfo about another subject"
    );
}
