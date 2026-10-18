//! share-upload spec.
mod common;
use common::*;

const HTML: &[u8] = b"<!doctype html><p>hi</p>";

async fn owner(app: &App) -> String {
    app.user("alice").await.1
}

fn settings(path: &str) -> Vec<(&str, &str)> {
    vec![("path", path), ("visibility", "public"), ("expiry", "never")]
}

#[tokio::test]
async fn single_markdown_file() {
    let app = App::new().await;
    let s = owner(&app).await;
    let res = app.upload(&s, &settings("notes"), &[("f:notes.md", b"# Notes")]).await;
    assert_eq!(res.status, 200, "{}", res.text());
    assert_eq!(res.json()["url"], format!("{SHARE}/notes/"));
    let entry: String = sqlx::query_scalar("SELECT entry FROM shares").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(entry, "notes.md");
}

#[tokio::test]
async fn zip_wrapped_folder_and_macos_junk() {
    let app = App::new().await;
    let s = owner(&app).await;
    let zip = zip_of(&[("site/index.html", HTML), ("site/img/a.png", b"png"), ("__MACOSX/site/._index.html", b"junk")]);
    let res = app.upload(&s, &settings("site"), &[("zip", &zip)]).await;
    assert_eq!(res.status, 200, "{}", res.text());
    let id = app.share_id("site").await;
    let keys = app.keys().await;
    let files: Vec<&str> =
        keys.iter().filter(|k| k.contains("/files/")).map(|k| k.split("/files/").nth(1).unwrap()).collect();
    assert_eq!(files, ["img/a.png", "index.html"], "{id}");
}

#[tokio::test]
async fn zip_dockerignore_in_wrapped_folder() {
    let app = App::new().await;
    let s = owner(&app).await;
    let zip = zip_of(&[
        ("site/.dockerignore", b"drafts\n!drafts/keep.md"),
        ("site/index.md", b"# hi"),
        ("site/drafts/a.md", b"x"),
        ("site/drafts/keep.md", b"x"),
    ]);
    assert_eq!(app.upload(&s, &settings("d"), &[("zip", &zip)]).await.status, 200);
    let keys = app.keys().await;
    assert!(keys.iter().any(|k| k.ends_with("files/drafts/keep.md")));
    assert!(!keys.iter().any(|k| k.ends_with("files/drafts/a.md")));
}

#[tokio::test]
async fn no_entry_file() {
    let app = App::new().await;
    let s = owner(&app).await;
    let res = app.upload(&s, &settings("x"), &[("f:report.html", HTML), ("f:README.md", b"x")]).await;
    assert_eq!(res.status, 400);
    assert!(res.json()["error"].as_str().unwrap().contains("index.html"));
    assert!(app.keys().await.is_empty());
}

#[tokio::test]
async fn unsafe_paths_and_limits() {
    let app = App::new().await;
    let s = owner(&app).await;
    for bad in ["f:../x.html", "f:/abs.html", "f:a//b.html", "f:a\\b.html"] {
        let res = app.upload(&s, &settings("x"), &[("f:index.html", HTML), (bad, b"x")]).await;
        assert_eq!(res.status, 400, "{bad}");
    }
    let slip = zip_of(&[("index.html", HTML), ("../../etc/passwd", b"x")]);
    assert_eq!(app.upload(&s, &settings("x"), &[("zip", &slip)]).await.status, 400);
    // The test limit is 1 MiB.
    let big = vec![b'a'; 1024 * 1024 + 1];
    assert_eq!(app.upload(&s, &settings("x"), &[("f:index.html", &big)]).await.status, 413);
    let bomb = zip_of(&[("index.html", &vec![0u8; 3 * 1024 * 1024])]);
    assert!(bomb.len() < 100 * 1024);
    assert_eq!(app.upload(&s, &settings("x"), &[("zip", &bomb)]).await.status, 413);
    assert!(app.keys().await.is_empty());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM shares").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn path_rules() {
    let app = App::new().await;
    let s = owner(&app).await;
    let res = app.upload(&s, &settings("Notes/2026/Q3"), &[("f:index.html", HTML)]).await;
    assert_eq!(res.json()["url"], format!("{SHARE}/notes/2026/q3/"));
    for bad in ["_dropgo/x", "docs/_x", "a b"] {
        assert_eq!(app.upload(&s, &settings(bad), &[("f:index.html", HTML)]).await.status, 400, "{bad}");
    }
    let random = app.upload(&s, &settings(""), &[("f:index.html", HTML)]).await;
    let url = random.json()["url"].as_str().unwrap().to_string();
    assert_eq!(url.trim_start_matches(&format!("{SHARE}/")).trim_end_matches('/').len(), 8);
}

#[tokio::test]
async fn nesting_conflict_names_the_share() {
    let app = App::new().await;
    let s = owner(&app).await;
    app.share(&s, "notes", "public", &[("f:index.html", HTML)]).await;
    let res = app.upload(&s, &settings("notes/2026"), &[("f:index.html", HTML)]).await;
    assert_eq!(res.status, 409);
    assert!(res.json()["error"].as_str().unwrap().contains("/notes/"));
    // Another owner cannot overwrite it either.
    let (_, bob) = app.user("bob").await;
    let res = app.upload(&bob, &settings("notes"), &[("f:index.html", HTML)]).await;
    assert_eq!(res.status, 409);
    assert!(res.json()["existing"].is_null());
}

#[tokio::test]
async fn expired_share_does_not_block() {
    let app = App::new().await;
    let s = owner(&app).await;
    let old = app.share(&s, "notes", "public", &[("f:index.html", HTML)]).await;
    sqlx::query("UPDATE shares SET expires_at = now() - interval '1 minute'").execute(&app.st.db).await.unwrap();
    let res = app.upload(&s, &settings("notes"), &[("f:index.html", HTML)]).await;
    assert_eq!(res.status, 200, "{}", res.text());
    let state: String =
        sqlx::query_scalar("SELECT state FROM shares WHERE id = $1").bind(old).fetch_one(&app.st.db).await.unwrap();
    assert_eq!(state, "trashed");
}

#[tokio::test]
async fn own_path_needs_confirmed_overwrite() {
    let app = App::new().await;
    let s = owner(&app).await;
    let id = app.share(&s, "notes", "password", &[("f:index.html", b"old")]).await;
    let res = app.upload(&s, &settings("notes"), &[("f:index.html", b"new")]).await;
    assert_eq!(res.status, 409);
    assert_eq!(res.json()["existing"]["id"], id.to_string());
    assert_eq!(res.json()["existing"]["visibility"], "password");

    // Confirmed, with the password left empty.
    let unlock = app
        .post_form(&format!("{SHARE}/_dropgo/unlock/{id}"), "", &[("password", "hunter2"), ("back", "/notes/")])
        .await
        .cookie(&format!("__Host-dropgo-unlock-{id}"))
        .unwrap();
    let ow = id.to_string();
    let res = app
        .upload(
            &s,
            &[("path", "notes"), ("visibility", "password"), ("expiry", "never"), ("overwrite", &ow)],
            &[("f:index.html", b"new")],
        )
        .await;
    assert_eq!(res.status, 200, "{}", res.text());
    assert_eq!(app.share_id("notes").await, id);
    // The old unlock cookie still works and serves the new files; the old prefix is gone.
    let page = app.get(&format!("{SHARE}/notes/index.html"), &[("cookie", &unlock)]).await;
    assert_eq!(page.text(), "new");
    assert_eq!(app.keys().await.iter().filter(|k| k.contains("/files/")).count(), 1);
}

#[tokio::test]
async fn overwrite_fails_partway_keeps_old_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = dropgo::store::from_url(&format!("file://{}", dir.path().display())).unwrap();
    let app = App::with(config(), store).await;
    let s = owner(&app).await;
    let id = app.share(&s, "notes", "public", &[("f:index.html", b"old")]).await;
    // Writes of a new upload under this share now fail.
    use std::os::unix::fs::PermissionsExt;
    let share_dir = dir.path().join(format!("shares/{id}"));
    std::fs::set_permissions(&share_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let ow = id.to_string();
    let res = app
        .upload(
            &s,
            &[("path", "notes"), ("visibility", "public"), ("expiry", "never"), ("overwrite", &ow)],
            &[("f:index.html", b"new"), ("f:b.png", b"b")],
        )
        .await;
    std::fs::set_permissions(&share_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(res.status, 500);
    assert_eq!(app.get(&format!("{SHARE}/notes/"), &[]).await.text(), "old");
}

#[tokio::test]
async fn two_uploads_at_once_one_wins() {
    let app = App::new().await;
    let s = owner(&app).await;
    let (sa, sb) = (settings("a"), settings("a/b"));
    let (a, b) =
        tokio::join!(app.upload(&s, &sa, &[("f:index.html", HTML)]), app.upload(&s, &sb, &[("f:index.html", HTML)]));
    let mut codes = [a.status, b.status];
    codes.sort();
    assert_eq!(codes, [200, 409]);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM shares").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 1);
    // The loser cleaned up its files.
    assert_eq!(app.keys().await.len(), 1);
}

#[tokio::test]
async fn two_overwrites_at_once() {
    let app = App::new().await;
    let s = owner(&app).await;
    let id = app.share(&s, "notes", "public", &[("f:index.html", b"v1")]).await;
    let ow = id.to_string();
    let set = [("path", "notes"), ("visibility", "public"), ("expiry", "never"), ("overwrite", ow.as_str())];
    let (a, b) = tokio::join!(
        app.upload(&s, &set, &[("f:index.html", b"v2")]),
        app.upload(&s, &set, &[("f:index.html", b"v3")])
    );
    assert_eq!((a.status, b.status), (200, 200));
    let upload: uuid::Uuid = sqlx::query_scalar("SELECT upload_id FROM shares").fetch_one(&app.st.db).await.unwrap();
    let keys = app.keys().await;
    assert_eq!(keys, [format!("shares/{id}/{upload}/files/index.html")]);
}

#[tokio::test]
async fn dropped_during_transfer_creates_nothing() {
    let app = App::new().await;
    let s = owner(&app).await;
    let (ct, body) = multipart(&settings("notes"), &[("f:index.html", HTML)]);
    // The client goes away halfway through the body.
    let half = bytes::Bytes::copy_from_slice(&body[..body.len() / 2]);
    let stream = futures::stream::iter([Ok::<_, std::io::Error>(half), Err(std::io::Error::other("reset"))]);
    let res = app
        .send(req(
            "POST",
            &format!("{ADMIN}/api/upload"),
            &[("cookie", &admin_cookie(&s)), ("origin", ADMIN), ("content-type", &ct)],
            axum::body::Body::from_stream(stream),
        ))
        .await;
    assert_eq!(res.status, 400);
    assert!(app.keys().await.is_empty());
    assert_eq!(app.upload(&s, &settings("notes"), &[("f:index.html", HTML)]).await.status, 200);
}

/// A store that is slow to write, so a test can act while the upload task runs.
fn slow_store() -> dropgo::store::Store {
    let cfg = object_store::throttle::ThrottleConfig {
        wait_put_per_call: std::time::Duration::from_millis(300),
        ..Default::default()
    };
    std::sync::Arc::new(object_store::throttle::ThrottledStore::new(object_store::memory::InMemory::new(), cfg))
}

#[tokio::test]
async fn dropped_after_transfer_still_creates_the_share() {
    let app = App::with(config(), slow_store()).await;
    let s = owner(&app).await;
    let app = std::sync::Arc::new(app);
    let a = app.clone();
    let s2 = s.clone();
    let req = tokio::spawn(async move { a.upload(&s2, &settings("notes"), &[("f:index.html", HTML)]).await });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    req.abort();
    dropgo::drain(&app.st).await;
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM shares WHERE path = 'notes'").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn shutdown_waits_for_upload_task() {
    let app = std::sync::Arc::new(App::with(config(), slow_store()).await);
    let s = owner(&app).await;
    let a = app.clone();
    let up =
        tokio::spawn(
            async move { a.upload(&s, &settings("notes"), &[("f:index.html", HTML), ("f:a.css", b"x")]).await },
        );
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    // What main does after the server stops taking requests.
    dropgo::drain(&app.st).await;
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM shares WHERE path = 'notes'").fetch_one(&app.st.db).await.unwrap();
    assert_eq!(n, 1);
    assert_eq!(up.await.unwrap().status, 200);
}

#[tokio::test]
async fn retry_while_first_is_written_gets_overwrite_prompt() {
    let app = std::sync::Arc::new(App::with(config(), slow_store()).await);
    let s = owner(&app).await;
    let (a, s1) = (app.clone(), s.clone());
    let first = tokio::spawn(async move { a.upload(&s1, &settings("notes"), &[("f:index.html", b"first")]).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // Passes the pre-check, then the first one becomes active before this claim.
    let second = app.upload(&s, &settings("notes"), &[("f:index.html", b"second"), ("f:b.css", b"x")]).await;
    assert_eq!(first.await.unwrap().status, 200);
    assert_eq!(second.status, 409);
    assert!(second.json()["existing"]["id"].is_string());
    assert_eq!(app.get(&format!("{SHARE}/notes/"), &[]).await.text(), "first");
}

#[tokio::test]
async fn path_check_endpoint() {
    let app = App::new().await;
    let s = owner(&app).await;
    let cookie = admin_cookie(&s);
    let id = app.share(&s, "notes", "public", &[("f:index.html", HTML)]).await;
    let free = app.get(&format!("{ADMIN}/api/check?path=other"), &[("cookie", &cookie)]).await;
    assert_eq!(free.status, 200);
    let own = app.get(&format!("{ADMIN}/api/check?path=Notes"), &[("cookie", &cookie)]).await;
    assert_eq!(own.status, 409);
    assert_eq!(own.json()["existing"]["id"], id.to_string());
    let nested = app.get(&format!("{ADMIN}/api/check?path=notes/x"), &[("cookie", &cookie)]).await;
    assert_eq!(nested.status, 409);
    assert!(nested.json()["existing"].is_null());
}
