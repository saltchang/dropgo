//! agent-api and api-tokens specs.
mod common;
use common::*;
use serde_json::json;

const MD: &[u8] = b"# Notes\n\nHello **agent**.";
const HTML: &[u8] = b"<p>x";

fn code(res: &Res) -> String {
    res.json()["error"]["code"].as_str().unwrap_or_default().to_string()
}

async fn up(app: &App, token: &str, path: &str, visibility: &str) -> serde_json::Value {
    let mut s = vec![("path", path), ("visibility", visibility), ("expiry", "never")];
    if visibility == "password" {
        s.push(("password", "hunter2"));
    }
    let res = app.api_upload(token, &s, &[("f:index.md", MD)]).await;
    assert_eq!(res.status, 201, "{}", res.text());
    res.json()
}

// ---------- bearer auth and errors ----------

#[tokio::test]
async fn cookie_alone_is_not_enough() {
    let app = App::new().await;
    let (_, s, _) = app.token("alice").await;
    let res = app.get(&format!("{ADMIN}/api/v1/shares"), &[("cookie", &admin_cookie(&s))]).await;
    assert_eq!((res.status, code(&res)), (401, "auth.missing_token".into()));
    let res = app.api("GET", "/me", "dgo_nope", None).await;
    assert_eq!((res.status, code(&res)), (401, "auth.invalid_token".into()));
}

#[tokio::test]
async fn bearer_scheme_is_case_insensitive() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let res = app.get(&format!("{ADMIN}/api/v1/me"), &[("authorization", &format!("bearer {t}"))]).await;
    assert_eq!(res.status, 200);
}

#[tokio::test]
async fn whoami_and_post_without_origin() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let me = app.api("GET", "/me", &t, None).await;
    assert_eq!(me.status, 200);
    assert_eq!(me.json()["name"], "test");
    assert!(me.json()["expires_at"].as_str().unwrap().ends_with('Z'));

    let id = up(&app, &t, "notes", "public").await["id"].as_str().unwrap().to_string();
    let res = app.api("POST", &format!("/shares/{id}/trash"), &t, None).await;
    assert_eq!((res.status, res.json()["state"].as_str()), (200, Some("trashed")));
}

#[tokio::test]
async fn error_shape_everywhere() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let res = app.api("GET", "/nope", &t, None).await;
    assert_eq!((res.status, code(&res)), (404, "api.not_found".into()));
    assert_eq!(res.json()["error"]["message"], "There is no such API route.");
    assert!(res.json()["error"]["details"].is_object());
    let res = app.api("GET", "/shares/not-a-uuid", &t, None).await;
    assert_eq!((res.status, code(&res)), (400, "api.bad_request".into()));
    let res = app.api("DELETE", "/shares", &t, None).await;
    assert_eq!((res.status, code(&res)), (405, "api.not_found".into()));
    let id = up(&app, &t, "a", "public").await["id"].as_str().unwrap().to_string();
    let res = app
        .send(req(
            "PATCH",
            &format!("{ADMIN}/api/v1/shares/{id}"),
            &[("authorization", &format!("Bearer {t}")), ("content-type", "application/json")],
            axum::body::Body::from("{not json"),
        ))
        .await;
    assert_eq!((res.status, code(&res)), (400, "api.bad_request".into()));
    // The purge route does not exist, and the share stays.
    let res = app.api("POST", &format!("/shares/{id}/purge"), &t, None).await;
    assert_eq!((res.status, code(&res)), (404, "api.not_found".into()));
    assert_eq!(app.api("GET", &format!("/shares/{id}"), &t, None).await.json()["state"], "active");
}

#[tokio::test]
async fn english_message_for_zh_tw_owner() {
    let app = App::new().await;
    let (uid, _, t) = app.token("alice").await;
    sqlx::query("UPDATE users SET locale = 'zh-TW' WHERE id = $1").bind(uid).execute(&app.st.db).await.unwrap();
    let big = vec![b'x'; 2 * 1024 * 1024];
    let res = app.api_upload(&t, &[("path", "big"), ("visibility", "public")], &[("f:index.html", &big)]).await;
    assert_eq!((res.status, code(&res)), (413, "upload.too_big".into()));
    assert_eq!(res.json()["error"]["message"], "The upload is over the size limit.");
}

// ---------- upload ----------

#[tokio::test]
async fn upload_markdown_and_share_object() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let s = up(&app, &t, "notes", "owner").await;
    assert_eq!(s["url"], format!("{SHARE}/notes/"));
    assert_eq!((s["visibility"].as_str(), s["state"].as_str()), (Some("owner"), Some("active")));
    assert_eq!((s["entry"].as_str(), s["file_count"].as_i64()), (Some("index.md"), Some(1)));
    assert!(s["expires_at"].is_null() && s["trashed_at"].is_null() && s["purge_at"].is_null());
    assert!(s.get("password_hash").is_none());
}

#[tokio::test]
async fn path_exists_and_overwrite() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let id = up(&app, &t, "demo", "public").await["id"].as_str().unwrap().to_string();
    let settings = [("path", "demo"), ("visibility", "public")];
    let res = app.api_upload(&t, &settings, &[("f:index.html", b"<p>new")]).await;
    assert_eq!((res.status, code(&res)), (409, "path.exists".into()));
    assert_eq!(res.json()["error"]["details"]["existing"]["id"], id.as_str());
    let settings = [("path", "demo"), ("visibility", "public"), ("overwrite", id.as_str())];
    let res = app.api_upload(&t, &settings, &[("f:index.html", b"<p>new")]).await;
    assert_eq!((res.status, res.json()["id"].as_str()), (201, Some(id.as_str())));
    assert!(app.get(&format!("{SHARE}/demo/"), &[]).await.text().contains("<p>new"));
}

#[tokio::test]
async fn settings_after_a_file() {
    let app = App::new().await;
    let (_, s, t) = app.token("alice").await;
    let (ct, mut body) = multipart(&[("visibility", "public")], &[("f:index.html", HTML)]);
    // Move the closing boundary after a trailing `path` field.
    body.truncate(body.len() - "------dropgo-test-boundary--\r\n".len());
    body.extend(b"------dropgo-test-boundary\r\nContent-Disposition: form-data; name=\"path\"\r\n\r\nlate\r\n");
    body.extend(b"------dropgo-test-boundary--\r\n");
    for (url, h) in
        [("/api/v1/shares", ("authorization", format!("Bearer {t}"))), ("/api/upload", ("cookie", admin_cookie(&s)))]
    {
        let res = app
            .send(req(
                "POST",
                &format!("{ADMIN}{url}"),
                &[(h.0, &h.1), ("origin", ADMIN), ("content-type", &ct)],
                axum::body::Body::from(body.clone()),
            ))
            .await;
        assert_eq!(res.status, 400, "{url}: {}", res.text());
        assert!(res.text().contains("upload.field_order") || res.text().contains("settings fields"), "{url}");
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM shares").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn large_upload() {
    let mut cfg = config();
    cfg.max_share_bytes = 100 * 1024 * 1024;
    let app = App::with(cfg, dropgo::store::from_url("memory://").unwrap()).await;
    let (_, _, t) = app.token("alice").await;
    let big = vec![b'x'; 50 * 1024 * 1024];
    let res = app
        .api_upload(&t, &[("path", "big"), ("visibility", "owner")], &[("f:index.html", HTML), ("f:big.bin", &big)])
        .await;
    assert_eq!(res.status, 201, "{}", res.text());
}

// ---------- check, list, get ----------

#[tokio::test]
async fn check_list_get() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let (_, _, bob) = app.token("bob").await;
    let mine = up(&app, &t, "notes/2026", "public").await["id"].as_str().unwrap().to_string();
    up(&app, &t, "other", "public").await;
    let theirs = up(&app, &bob, "bobs", "public").await["id"].as_str().unwrap().to_string();

    let res = app.api("GET", "/check?path=Fresh/", &t, None).await;
    assert_eq!(res.json(), json!({ "ok": true, "path": "fresh" }));
    let res = app.api("GET", "/check?path=notes/2026", &t, None).await;
    assert_eq!((res.status, code(&res)), (409, "path.exists".into()));

    let list = app.api("GET", "/shares?path=Notes/2026/", &t, None).await.json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], mine.as_str());
    assert_eq!(app.api("GET", "/shares", &t, None).await.json().as_array().unwrap().len(), 2);

    let res = app.api("GET", &format!("/shares/{theirs}"), &t, None).await;
    assert_eq!((res.status, code(&res)), (404, "share.not_found".into()));
}

// ---------- edit ----------

#[tokio::test]
async fn edit_rules() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let id = up(&app, &t, "pw", "password").await["id"].as_str().unwrap().to_string();
    let url = format!("/shares/{id}");
    let hash = || async {
        sqlx::query_scalar::<_, Option<String>>("SELECT password_hash FROM shares WHERE path = 'pw'")
            .fetch_one(&app.st.db)
            .await
            .unwrap()
    };

    // Only the expiry changes.
    let soon = (time::OffsetDateTime::now_utc() + time::Duration::days(2))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    app.api("PATCH", &url, &t, Some(json!({ "expires_at": soon }))).await;
    let before = hash().await;
    let res = app.api("PATCH", &url, &t, Some(json!({ "expires_at": null }))).await;
    let s = res.json();
    assert_eq!((s["path"].as_str(), s["visibility"].as_str()), (Some("pw"), Some("password")));
    assert!(s["expires_at"].is_null());
    assert!(before.is_some() && hash().await == before);

    for bad in ["2000-01-01T00:00:00Z", "keep", ""] {
        let res = app.api("PATCH", &url, &t, Some(json!({ "expires_at": bad }))).await;
        assert_eq!((res.status, code(&res)), (400, "form.bad_expiry".into()), "{bad}");
    }

    // Public clears the password.
    let res = app.api("PATCH", &url, &t, Some(json!({ "visibility": "public" }))).await;
    assert_eq!(res.json()["visibility"], "public");
    assert_eq!(hash().await, None);
    assert_eq!(app.get(&format!("{SHARE}/pw/"), &[]).await.status, 200);

    // A password alone does not make a public share a password share.
    let res = app.api("PATCH", &url, &t, Some(json!({ "password": "x" }))).await;
    assert_eq!((res.status, code(&res)), (400, "form.password_unused".into()));
    let res = app.api("PATCH", &url, &t, Some(json!({ "visibility": "password" }))).await;
    assert_eq!((res.status, code(&res)), (400, "form.password_required".into()));
    let res = app.api("PATCH", &url, &t, Some(json!({ "visibility": "password", "password": "x" }))).await;
    assert_eq!(res.json()["visibility"], "password");

    up(&app, &t, "taken", "public").await;
    let res = app.api("PATCH", &url, &t, Some(json!({ "path": "taken/sub" }))).await;
    assert_eq!((res.status, code(&res)), (409, "path.conflict".into()));
    assert_eq!(res.json()["error"]["details"]["arg"], "taken");
}

// ---------- trash and restore ----------

#[tokio::test]
async fn trash_and_restore() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let id = up(&app, &t, "notes", "public").await["id"].as_str().unwrap().to_string();
    let first = app.api("POST", &format!("/shares/{id}/trash"), &t, None).await.json();
    let second = app.api("POST", &format!("/shares/{id}/trash"), &t, None).await;
    assert_eq!(second.status, 200);
    assert_eq!(second.json()["trashed_at"], first["trashed_at"]);
    let trashed = time::OffsetDateTime::parse(
        first["trashed_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let purge = time::OffsetDateTime::parse(
        first["purge_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert_eq!(purge - trashed, time::Duration::days(30));
    let got = app.api("GET", &format!("/shares/{id}"), &t, None).await.json();
    assert_eq!(got["state"], "trashed");
    assert_eq!(app.api("GET", "/shares?state=trash", &t, None).await.json().as_array().unwrap().len(), 1);

    up(&app, &t, "notes", "public").await;
    let res = app.api("POST", &format!("/shares/{id}/restore"), &t, None).await;
    assert_eq!((res.status, code(&res)), (409, "restore.conflict".into()));
    let res = app.api("POST", &format!("/shares/{id}/restore"), &t, Some(json!({ "path": "notes-2" }))).await;
    assert_eq!((res.status, res.json()["path"].as_str()), (200, Some("notes-2")));
    let res = app.api("POST", &format!("/shares/{id}/restore"), &t, None).await;
    assert_eq!((res.status, code(&res)), (409, "share.not_trashed".into()));
    let res = app.api("POST", &format!("/shares/{}/restore", uuid::Uuid::new_v4()), &t, None).await;
    assert_eq!((res.status, code(&res)), (404, "share.not_found".into()));
}

// ---------- files ----------

#[tokio::test]
async fn read_files_of_an_owner_only_share() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    let id = up(&app, &t, "private", "owner").await["id"].as_str().unwrap().to_string();
    let files = app.api("GET", &format!("/shares/{id}/files"), &t, None).await.json();
    assert_eq!(files, json!([{ "path": "index.md", "size": MD.len() }]));

    let raw = app.api("GET", &format!("/shares/{id}/files/index.md"), &t, None).await;
    assert_eq!(raw.body.as_ref(), MD);
    let res = app.api("GET", &format!("/shares/{id}/files/index.md?rendered=1"), &t, None).await;
    assert_eq!(res.status, 200);
    assert!(res.text().contains("<strong>agent</strong>"), "{}", res.text());
    assert!(res.header("content-type").starts_with("text/html"));
    assert!(res.header("content-security-policy").starts_with("sandbox"));
    assert_eq!(res.header("x-content-type-options"), "nosniff");
    assert_eq!(res.header("content-disposition"), "attachment");

    let res = app.api("GET", &format!("/shares/{id}/files/nope.md"), &t, None).await;
    assert_eq!((res.status, code(&res)), (404, "file.not_found".into()));
}

// ---------- tokens ----------

#[tokio::test]
async fn token_shown_once() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let c = admin_cookie(&s);
    let res = app.post_form(&format!("{ADMIN}/tokens"), &c, &[("name", "laptop"), ("days", "90")]).await;
    assert_eq!(res.status, 200);
    assert_eq!(res.header("cache-control"), "no-store");
    let html = res.text();
    let token = html
        .split("id=\"new-token\"")
        .nth(1)
        .unwrap()
        .split("value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    assert!(token.starts_with("dgo_") && token.len() > 40, "{token}");
    assert_eq!(app.api("GET", "/me", &token, None).await.json()["name"], "laptop");

    let page = app.get(&format!("{ADMIN}/tokens"), &[("cookie", &c)]).await;
    assert_eq!(page.header("cache-control"), "no-store");
    let html = page.text();
    assert!(html.contains("laptop") && !html.contains(&token));
    assert!(html.contains(&format!("dgo_{}", &token[4..12])));
    let expires: time::OffsetDateTime =
        sqlx::query_scalar("SELECT expires_at FROM api_tokens").fetch_one(&app.st.db).await.unwrap();
    let days = (expires - time::OffsetDateTime::now_utc()).whole_hours();
    assert!((89 * 24..=90 * 24).contains(&days), "{days}");
    // The stored hash is not the token.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT hash FROM api_tokens").fetch_one(&app.st.db).await.unwrap();
    assert_ne!(stored, token.as_bytes());
}

#[tokio::test]
async fn name_required_and_claim_required() {
    let app = App::new().await;
    let (id, s) = app.user("alice").await;
    let c = admin_cookie(&s);
    let res = app.post_form(&format!("{ADMIN}/tokens"), &c, &[("name", " "), ("days", "90")]).await;
    assert!(res.text().contains("Enter a name for the token."));
    // A session from before owner claims were stored signs in again (the provider is down in tests, so 502).
    sqlx::query("UPDATE users SET owner_claim = NULL WHERE id = $1").bind(id).execute(&app.st.db).await.unwrap();
    assert_eq!(app.get(&format!("{ADMIN}/tokens"), &[("cookie", &c)]).await.status, 502);
    let res = app.post_form(&format!("{ADMIN}/tokens"), &c, &[("name", "laptop"), ("days", "90")]).await;
    assert_eq!(res.status, 401);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM api_tokens").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn revoke_last_used_and_scoping() {
    let app = App::new().await;
    let (_, s, t) = app.token("alice").await;
    let (_, bob_s, bob_t) = app.token("bob").await;
    let db = app.st.db.clone();
    let tid = |user: &'static str| {
        let db = db.clone();
        async move {
            sqlx::query_scalar::<_, uuid::Uuid>(
                "SELECT t.id FROM api_tokens t JOIN users u ON u.id = t.user_id WHERE u.owner_claim = $1",
            )
            .bind(user)
            .fetch_one(&db)
            .await
            .unwrap()
        }
    };
    let before = app.get(&format!("{ADMIN}/tokens"), &[("cookie", &admin_cookie(&s))]).await.text();
    assert!(before.contains("never"));
    app.api("GET", "/me", &t, None).await;
    let after = app.get(&format!("{ADMIN}/tokens"), &[("cookie", &admin_cookie(&s))]).await.text();
    assert!(!after.contains(">never<") && after.matches("<time").count() == 3, "{after}");

    // Alice cannot revoke Bob's token, and Bob does not see Alice's.
    let bobs = tid("bob").await;
    app.post_form(&format!("{ADMIN}/tokens/{bobs}/revoke"), &admin_cookie(&s), &[]).await;
    assert_eq!(app.api("GET", "/me", &bob_t, None).await.status, 200);
    let bob_page = app.get(&format!("{ADMIN}/tokens"), &[("cookie", &admin_cookie(&bob_s))]).await.text();
    assert_eq!(bob_page.matches("class=\"share\"").count(), 1);

    let res = app.post_form(&format!("{ADMIN}/tokens/{}/revoke", tid("alice").await), &admin_cookie(&s), &[]).await;
    assert_eq!(res.status, 303);
    let res = app.api("GET", "/me", &t, None).await;
    assert_eq!((res.status, code(&res)), (401, "auth.invalid_token".into()));
}

#[tokio::test]
async fn expired_token_and_allowlist() {
    let app = App::new().await;
    let (_, _, t) = app.token("alice").await;
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '1 second'").execute(&app.st.db).await.unwrap();
    let res = app.api("GET", "/me", &t, None).await;
    assert_eq!((res.status, code(&res)), (401, "auth.token_expired".into()));

    // Same database, restarted with alice off the allowlist.
    let (_, s2, t2) = app.token("alice2").await;
    let mut cfg = config();
    cfg.owners = vec!["alice".into()];
    let st = dropgo::Inner::new(cfg, app.st.db.clone(), app.st.store.clone()).await.unwrap();
    let restarted = App { router: dropgo::router(st.clone()), st };
    let res = restarted.api("GET", "/me", &t2, None).await;
    assert_eq!((res.status, code(&res)), (401, "auth.invalid_token".into()));
    // The 30-day session stops working at once too.
    assert_eq!(restarted.get(&format!("{ADMIN}/"), &[("cookie", &admin_cookie(&s2))]).await.status, 502);
}
