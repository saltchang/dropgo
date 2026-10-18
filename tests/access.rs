//! share-access and markdown-rendering specs.
mod common;
use common::*;
use dropgo::token::{self, Kind};

const HTML: &[u8] = b"<!doctype html><img src=a.png>";

fn unlock_cookie(res: &Res, id: uuid::Uuid) -> String {
    res.cookie(&format!("__Host-dropgo-unlock-{id}")).expect("unlock cookie")
}

#[tokio::test]
async fn public_owner_only_and_other_user() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    app.share(&s, "pub", "public", &[("f:index.html", HTML)]).await;
    app.share(&s, "mine", "owner", &[("f:index.html", HTML), ("f:a.png", b"png")]).await;

    let res = app.nav(&format!("{SHARE}/pub/"), "").await;
    assert_eq!(res.status, 200);
    assert_eq!(res.body.as_ref(), HTML, "HTML is served unchanged");
    assert!(res.header("x-robots-tag").contains("noindex"));
    assert_eq!(res.header("access-control-allow-origin"), "*");
    assert!(res.header("content-security-policy").starts_with("sandbox allow-scripts"));
    assert_eq!(res.header("referrer-policy"), "no-referrer");

    // Signed out: sign-in starts (the provider is down in tests, so 502), subresource: 401.
    assert_eq!(app.nav(&format!("{SHARE}/mine/"), "").await.status, 502);
    assert_eq!(app.get(&format!("{SHARE}/mine/a.png"), &[]).await.status, 401);

    let (_, bob) = app.user("bob").await;
    assert_eq!(app.nav(&format!("{SHARE}/mine/"), &share_cookie(&bob)).await.status, 404);

    // The owner gets a token URL, and its subresources load without cookies.
    let res = app.nav(&format!("{SHARE}/mine/"), &share_cookie(&s)).await;
    assert_eq!(res.status, 303);
    let loc = res.header("location").to_string();
    assert!(loc.starts_with("/_dropgo/t/") && loc.ends_with("/mine/"), "{loc}");
    assert_eq!(app.nav(&format!("{SHARE}{loc}"), "").await.status, 200);
    let img = loc.replace("/mine/", "/mine/a.png");
    assert_eq!(app.get(&format!("{SHARE}{img}"), &[]).await.body.as_ref(), b"png");
}

#[tokio::test]
async fn password_flow() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let id = app
        .share(&s, "doc", "password", &[("f:index.html", HTML), ("f:report.pdf", b"%PDF"), ("f:a.png", b"png")])
        .await;

    // Navigation gets the form; also when only Accept says HTML.
    let form = app.nav(&format!("{SHARE}/doc/report.pdf"), "").await;
    assert_eq!(form.status, 401);
    assert!(form.text().contains(&format!("/_dropgo/unlock/{id}")));
    assert!(form.header("content-security-policy").starts_with("default-src 'none'"));
    assert_eq!(app.get(&format!("{SHARE}/doc/"), &[("accept", "text/html")]).await.status, 401);
    assert_eq!(app.get(&format!("{SHARE}/doc/a.png"), &[]).await.status, 401);

    let wrong = app
        .post_form(&format!("{SHARE}/_dropgo/unlock/{id}"), "", &[("password", "nope"), ("back", "/doc/report.pdf")])
        .await;
    assert_eq!(wrong.status, 401);
    let ok = app
        .post_form(&format!("{SHARE}/_dropgo/unlock/{id}"), "", &[("password", "hunter2"), ("back", "/doc/report.pdf")])
        .await;
    assert_eq!(ok.status, 303);
    let loc = ok.header("location").to_string();
    assert!(loc.starts_with("/_dropgo/t/") && loc.ends_with("/doc/report.pdf"), "{loc}");
    let cookie = unlock_cookie(&ok, id);
    assert!(ok.header("set-cookie").contains("SameSite=None"));

    // Direct file link lands on the file; the download needs no form.
    assert_eq!(app.nav(&format!("{SHARE}{loc}"), "").await.body.as_ref(), b"%PDF");
    let dl = app.nav(&format!("{SHARE}/_dropgo/download/{id}?format=zip"), &cookie).await;
    assert_eq!(dl.status, 200);
    assert!(dl.header("content-disposition").starts_with("attachment"));

    // The owner skips the form.
    assert_eq!(app.nav(&format!("{SHARE}/doc/"), &share_cookie(&s)).await.status, 303);

    // Changing the password invalidates the unlock cookie and the token URLs.
    let edit = app
        .post_form(
            &format!("{ADMIN}/shares/{id}/edit"),
            &admin_cookie(&s),
            &[("path", "doc"), ("visibility", "password"), ("password", "new-one"), ("expires_at", "keep")],
        )
        .await;
    assert_eq!(edit.status, 303, "{}", edit.text());
    assert_eq!(app.nav(&format!("{SHARE}/doc/"), &cookie).await.status, 401);
    let token_img = loc.replace("/doc/report.pdf", "/doc/a.png");
    assert_eq!(app.get(&format!("{SHARE}{token_img}"), &[]).await.status, 401);
}

#[tokio::test]
async fn sixth_wrong_attempt_is_limited() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let id = app.share(&s, "doc", "password", &[("f:index.html", HTML)]).await;
    let url = format!("{SHARE}/_dropgo/unlock/{id}");
    for _ in 0..5 {
        assert_eq!(app.post_form(&url, "", &[("password", "x"), ("back", "/doc/")]).await.status, 401);
    }
    assert_eq!(app.post_form(&url, "", &[("password", "hunter2"), ("back", "/doc/")]).await.status, 429);
}

#[tokio::test]
async fn concurrent_guesses_share_the_limit() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let id = app.share(&s, "doc", "password", &[("f:index.html", HTML)]).await;
    let url = format!("{SHARE}/_dropgo/unlock/{id}");
    let guesses = (0..20).map(|_| app.post_form(&url, "", &[("password", "x"), ("back", "/doc/")]));
    let checked = futures::future::join_all(guesses).await.iter().filter(|r| r.status == 401).count();
    assert!(checked <= 5, "{checked} guesses were checked");
}

#[tokio::test]
async fn tokens() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let a = app.share(&s, "a", "owner", &[("f:index.html", HTML)]).await;
    app.share(&s, "b", "owner", &[("f:index.html", HTML), ("f:x.png", b"x")]).await;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let for_a = token::sign(&app.st.cfg.secret, Kind::Path, a, "owner", now);
    // Token for another share.
    assert_eq!(app.get(&format!("{SHARE}/_dropgo/t/{for_a}/b/x.png"), &[]).await.status, 401);
    let nav = app.nav(&format!("{SHARE}/_dropgo/t/{for_a}/b/"), "").await;
    assert_eq!((nav.status, nav.header("location")), (303, "/b/"));
    // A bad token never redirects off the share host.
    let nav = app.nav(&format!("{SHARE}/_dropgo/t/bad//b%2F@evil.example"), "").await;
    assert!(!nav.header("location").starts_with("//"), "{}", nav.header("location"));
    // Issued 13 hours ago.
    let old = token::sign(&app.st.cfg.secret, Kind::Path, a, "owner", now - 13 * 3600);
    let nav = app.nav(&format!("{SHARE}/_dropgo/t/{old}/a/"), &share_cookie(&s)).await;
    assert_eq!((nav.status, nav.header("location")), (303, "/a/"));
    assert!(app.nav(&format!("{SHARE}/a/"), &share_cookie(&s)).await.header("location").starts_with("/_dropgo/t/"));
    // Public shares never redirect.
    app.share(&s, "p", "public", &[("f:index.html", HTML)]).await;
    assert_eq!(app.nav(&format!("{SHARE}/p/"), "").await.status, 200);
}

#[tokio::test]
async fn lookup_rules() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let files: &[(&str, &[u8])] = &[("f:index.html", HTML), ("f:Img/A.png", b"A"), ("f:2026/index.html", b"dir")];
    app.share(&s, "notes/2026", "public", files).await;

    let res = app.get(&format!("{SHARE}/notes/2026"), &[]).await;
    assert_eq!((res.status, res.header("location")), (301, "/notes/2026/"));
    assert_eq!(app.get(&format!("{SHARE}/Notes/2026/Img/A.png"), &[]).await.body.as_ref(), b"A");
    assert_eq!(app.get(&format!("{SHARE}/notes/2026/img/a.png"), &[]).await.status, 404, "file case is kept");
    assert_eq!(app.get(&format!("{SHARE}/nothing/"), &[]).await.status, 404);

    // Expired before the job ran: 410.
    sqlx::query("UPDATE shares SET expires_at = now() - interval '1 second'").execute(&app.st.db).await.unwrap();
    assert_eq!(app.get(&format!("{SHARE}/notes/2026/"), &[]).await.status, 410);

    // An active share at `notes` beats the trashed `notes/2026`.
    app.share(&s, "notes", "public", files).await;
    let res = app.get(&format!("{SHARE}/notes/2026/"), &[]).await;
    assert_eq!((res.status, res.text().as_str()), (200, "dir"));
    let sub = app.get(&format!("{SHARE}/notes/2026"), &[]).await;
    assert_eq!((sub.status, sub.header("location")), (301, "/notes/2026/"), "directory without slash");
    let sub = app.get(&format!("{SHARE}//notes/2026"), &[]).await;
    assert_eq!((sub.status, sub.header("location")), (301, "/notes/2026/"), "never a scheme-relative redirect");

    // Purged: 404.
    sqlx::query("DELETE FROM shares WHERE path = 'notes'").execute(&app.st.db).await.unwrap();
    sqlx::query("DELETE FROM shares WHERE path = 'notes/2026'").execute(&app.st.db).await.unwrap();
    assert_eq!(app.get(&format!("{SHARE}/notes/2026/"), &[]).await.status, 404);
}

#[tokio::test]
async fn markdown_pages() {
    let app = App::new().await;
    let (uid, s) = app.user("alice").await;
    let md =
        b"# Title\n\n[guide](guide.md)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n$x^2$\n";
    let id = app.share(&s, "md", "public", &[("f:index.md", md), ("f:guide.md", b"# Guide")]).await;

    let page = app.nav(&format!("{SHARE}/md/"), "").await;
    let html = page.text();
    assert_eq!(page.status, 200);
    assert!(html.starts_with(r#"<!doctype html><html data-theme="dark">"#), "dark by default");
    assert!(html.contains("<table>") && html.contains(r#"<span class="hl-source hl-rust">"#), "{html}");
    assert!(html.contains("katex.min.js") && !html.contains("mermaid.min.js"));
    assert!(html.contains(&format!("/_dropgo/download/{id}?format=html")));
    assert!(page.header("cache-control").contains("private"));
    assert!(app.nav(&format!("{SHARE}/md/guide.md"), "").await.text().contains("<h1>Guide</h1>"));

    // Single-file share.
    app.share(&s, "one", "public", &[("f:notes.md", b"# Notes")]).await;
    assert!(app.nav(&format!("{SHARE}/one/"), "").await.text().contains("<h1>Notes</h1>"));

    // Theme: visitor cookie, then the owner's saved theme.
    let t = app.get(&format!("{SHARE}/_dropgo/theme?set=light&back=/md/"), &[]).await;
    assert_eq!((t.status, t.header("location")), (303, "/md/"));
    let theme = t.cookie("__Host-dropgo-theme").unwrap();
    assert!(app.nav(&format!("{SHARE}/one/"), &theme).await.text().contains(r#"data-theme="light""#));
    sqlx::query("UPDATE users SET theme = 'light' WHERE id = $1").bind(uid).execute(&app.st.db).await.unwrap();
    assert!(app.nav(&format!("{SHARE}/md/"), &share_cookie(&s)).await.text().contains(r#"data-theme="light""#));
    let evil = app.get(&format!("{SHARE}/_dropgo/theme?set=light&back=//evil.example/"), &[]).await;
    assert_eq!(evil.header("location"), "/");
}

#[tokio::test]
async fn cross_site_theme_toggle_keeps_saved_theme() {
    let app = App::new().await;
    let (uid, s) = app.user("alice").await;
    app.get(&format!("{SHARE}/_dropgo/theme?set=light"), &[("cookie", &share_cookie(&s))]).await;
    let theme: String =
        sqlx::query_scalar("SELECT theme FROM users WHERE id = $1").bind(uid).fetch_one(&app.st.db).await.unwrap();
    assert_eq!(theme, "dark");
}

#[tokio::test]
async fn downloads() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let md = b"# T\n\n<script>alert(1)</script>\n";
    let id = app
        .share(&s, "md", "public", &[("f:index.md", md), ("f:img/a.png", b"png"), ("f:img/100% #1.png", b"pct")])
        .await;

    let zip = app.get(&format!("{SHARE}/_dropgo/download/{id}?format=zip"), &[]).await;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip.body.to_vec())).unwrap();
    let mut names: Vec<String> =
        (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().unwrap().to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        ["img/100% #1.png", "img/a.png", "index.md"],
        "names the store percent-encodes come back decoded"
    );

    let html = app.get(&format!("{SHARE}/_dropgo/download/{id}?format=html"), &[]).await;
    assert!(html.header("content-disposition").starts_with("attachment; filename=\"index.html\""));
    assert!(html.header("content-security-policy").starts_with("sandbox"));
    let text = html.text();
    assert!(text.contains(r#"data-theme="dark""#) && text.contains("<style>") && !text.contains("<footer"));
    assert!(!text.contains("s.test") && !text.contains("/_dropgo/"), "no reference to the share host");

    // `source` is only for single-file shares.
    assert_eq!(app.get(&format!("{SHARE}/_dropgo/download/{id}?format=source"), &[]).await.status, 404);

    // Owner-only download sends a signed-out visitor to sign-in.
    let own = app.share(&s, "own", "owner", &[("f:notes.md", b"# x")]).await;
    assert_eq!(app.nav(&format!("{SHARE}/_dropgo/download/{own}?format=source"), "").await.status, 502);
    let src = app.nav(&format!("{SHARE}/_dropgo/download/{own}?format=source"), &share_cookie(&s)).await;
    assert_eq!(src.body.as_ref(), b"# x");
}

#[tokio::test]
async fn rerender_and_restore_use_new_template() {
    let app = App::new().await;
    let (uid, s) = app.user("alice").await;
    let a = app.share(&s, "a", "public", &[("f:index.md", b"# A")]).await;
    let b = app.share(&s, "b", "public", &[("f:index.md", b"# B")]).await;
    dropgo::lifecycle::delete(&app.st, uid, b, time::OffsetDateTime::now_utc()).await.unwrap();
    // Pretend the stored pages came from an old template.
    for id in [a, b] {
        let upload: uuid::Uuid = sqlx::query_scalar("SELECT upload_id FROM shares WHERE id = $1")
            .bind(id)
            .fetch_one(&app.st.db)
            .await
            .unwrap();
        let key = dropgo::store::rendered_key(id, upload, "index.md");
        object_store::ObjectStoreExt::put(&app.st.store, &key, "old".into()).await.unwrap();
    }
    dropgo::rerender(&app.st).await.unwrap();
    dropgo::lifecycle::restore(&app.st, uid, b, None, time::OffsetDateTime::now_utc()).await.unwrap().unwrap();
    assert!(app.get(&format!("{SHARE}/a/"), &[]).await.text().contains("<h1>A</h1>"));
    assert!(app.get(&format!("{SHARE}/b/"), &[]).await.text().contains("<h1>B</h1>"));
}

#[tokio::test]
async fn font_in_uploaded_page_is_cors_readable() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    app.share(&s, "f", "public", &[("f:index.html", HTML), ("f:font.woff2", b"wOF2")]).await;
    let res = app.get(&format!("{SHARE}/f/font.woff2"), &[("origin", "null"), ("sec-fetch-mode", "cors")]).await;
    assert_eq!((res.status, res.header("access-control-allow-origin")), (200, "*"));
    let css = app.get(&format!("{SHARE}/_dropgo/assets/vendor/katex/katex.min.css"), &[]).await;
    assert_eq!((css.status, css.header("access-control-allow-origin")), (200, "*"));
}
