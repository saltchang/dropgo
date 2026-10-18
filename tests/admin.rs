//! admin-ui spec, and the origin rules of owner-auth.
mod common;
use common::*;

const HTML: &[u8] = b"<p>x";

#[tokio::test]
async fn upload_page_defaults() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let res = app.get(&format!("{ADMIN}/"), &[("cookie", &admin_cookie(&s))]).await;
    assert_eq!(res.status, 200);
    let html = res.text();
    assert!(
        html.contains(r#"<option value="owner" selected>"#)
            && html.contains(r#"<input type="datetime-local" id="expires-local">"#)
    );
    assert!(res.header("content-security-policy").contains("script-src 'self'"));
    assert!(res.header("x-robots-tag").contains("noindex"));
}

#[tokio::test]
async fn list_and_edit_path() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let c = admin_cookie(&s);
    let id = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    app.share(&s, "notes-2026", "public", &[("f:index.html", HTML)]).await;
    // The search runs in admin.js over what each row shows, entry file and type included.
    let list = app.get(&format!("{ADMIN}/shares"), &[("cookie", &c)]).await.text();
    assert!(
        list.contains("/notes-2026/")
            && list.contains("index.html</span>")
            && list.contains(r#"<span class="badge">HTML"#)
    );

    let res = app
        .post_form(
            &format!("{ADMIN}/shares/{id}/edit"),
            &c,
            &[("path", "b"), ("visibility", "public"), ("expires_at", "keep")],
        )
        .await;
    assert_eq!(res.status, 303);
    assert_eq!(app.get(&format!("{SHARE}/b/"), &[]).await.status, 200);
    assert_eq!(app.get(&format!("{SHARE}/a/"), &[]).await.status, 404);

    // The upload path rules apply.
    let res = app
        .post_form(
            &format!("{ADMIN}/shares/{id}/edit"),
            &c,
            &[("path", "notes-2026/x"), ("visibility", "public"), ("expires_at", "keep")],
        )
        .await;
    assert_eq!(res.status, 200);
    assert!(res.text().contains("/notes-2026/"));
    let res = app
        .post_form(
            &format!("{ADMIN}/shares/{id}/edit"),
            &c,
            &[("path", "b"), ("visibility", "password"), ("expires_at", "keep")],
        )
        .await;
    assert!(res.text().contains("needs a password"));
}

#[tokio::test]
async fn edit_password_and_expiry() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let c = admin_cookie(&s);
    let id = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    let url = format!("{ADMIN}/shares/{id}/edit");
    let row = || async {
        sqlx::query_as::<_, (String, bool, Option<time::OffsetDateTime>)>(
            "SELECT visibility, password_hash IS NOT NULL, expires_at FROM shares WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&app.st.db)
        .await
        .unwrap()
    };
    let soon = time::OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() + time::Duration::days(2);
    let soon_text = soon.format(&time::format_description::well_known::Rfc3339).unwrap();

    // A password turns a public share into a password share.
    let f = [("path", "a"), ("visibility", "public"), ("password", "pw"), ("expires_at", &soon_text)];
    assert_eq!(app.post_form(&url, &c, &f).await.status, 303);
    assert_eq!(row().await, ("password".into(), true, Some(soon)));
    // An empty password and `keep` change nothing.
    let f = [("path", "a"), ("visibility", "public"), ("expires_at", "keep")];
    assert_eq!(app.post_form(&url, &c, &f).await.status, 303);
    assert_eq!(row().await, ("password".into(), true, Some(soon)));
    // Remove the password and clear the expiry.
    let f = [("path", "a"), ("visibility", "public"), ("clear_password", "1"), ("expires_at", "")];
    assert_eq!(app.post_form(&url, &c, &f).await.status, 303);
    assert_eq!(row().await, ("public".into(), false, None));
    // Owner-only drops a typed password.
    let f = [("path", "a"), ("visibility", "owner"), ("password", "pw"), ("expires_at", "keep")];
    assert_eq!(app.post_form(&url, &c, &f).await.status, 303);
    assert_eq!(row().await, ("owner".into(), false, None));
}

#[tokio::test]
async fn cross_site_posts_are_refused() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let id = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    let url = format!("{ADMIN}/shares/{id}/delete");
    for origin in [Some(SHARE), Some("null"), None] {
        let mut h = vec![("cookie", admin_cookie(&s))];
        if let Some(o) = origin {
            h.push(("origin", o.to_string()));
        }
        let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let res = app.send(req("POST", &url, &h, axum::body::Body::empty())).await;
        assert_eq!(res.status, 403, "{origin:?}");
    }
    assert_eq!(app.get(&format!("{SHARE}/a/"), &[]).await.status, 200, "nothing changed");
}

#[tokio::test]
async fn preferences_follow_the_user() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let res =
        app.post_form(&format!("{ADMIN}/prefs"), &admin_cookie(&s), &[("theme", "light"), ("back", "/shares")]).await;
    assert_eq!((res.status, res.header("location")), (303, "/shares"));
    app.post_form(&format!("{ADMIN}/prefs"), &admin_cookie(&s), &[("locale", "zh-TW"), ("back", "//evil")]).await;
    // Another device: a new session for the same user.
    let (_, other) = app.user("alice").await;
    let html = app.get(&format!("{ADMIN}/"), &[("cookie", &admin_cookie(&other))]).await.text();
    assert!(html.contains(r#"lang="zh-TW" data-theme="light""#), "{html}");
    assert!(html.contains("上傳"));
}

#[tokio::test]
async fn signed_out_admin_needs_sign_in() {
    let app = App::new().await;
    // The provider is unreachable in this test; sign-in tests use a mock provider.
    assert_eq!(app.get(&format!("{ADMIN}/"), &[]).await.status, 502);
    let res = app.post_form(&format!("{ADMIN}/prefs"), "", &[("theme", "light")]).await;
    assert_eq!(res.status, 401);
    assert_eq!(app.get(&format!("{ADMIN}/_dropgo/assets/dropgo.css"), &[]).await.status, 200);
}
