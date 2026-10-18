//! share-lifecycle spec. The job takes `now`, so the clock is injected.
mod common;
use common::*;
use dropgo::lifecycle::{self, RestoreError};
use time::{Duration, OffsetDateTime};

const HTML: &[u8] = b"<p>x";

async fn state(app: &App, id: uuid::Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT state FROM shares WHERE id = $1").bind(id).fetch_optional(&app.st.db).await.unwrap()
}

#[tokio::test]
async fn expiry_presets_and_custom_zone() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let before = OffsetDateTime::now_utc();
    app.upload(&s, &[("path", "a"), ("visibility", "public"), ("expiry", "7d")], &[("f:index.html", HTML)]).await;
    let e: OffsetDateTime = sqlx::query_scalar("SELECT expires_at FROM shares").fetch_one(&app.st.db).await.unwrap();
    assert!((e - before - Duration::days(7)).abs() < Duration::seconds(30));

    let res = app
        .upload(
            &s,
            &[
                ("path", "b"),
                ("visibility", "public"),
                ("expiry", "custom"),
                ("expires_at", "2099-03-01T18:00:00+08:00"),
            ],
            &[("f:index.html", HTML)],
        )
        .await;
    assert_eq!(res.status, 200, "{}", res.text());
    let e: OffsetDateTime =
        sqlx::query_scalar("SELECT expires_at FROM shares WHERE path = 'b'").fetch_one(&app.st.db).await.unwrap();
    assert_eq!((e.hour(), e.offset().is_utc()), (10, true));
}

#[tokio::test]
async fn job_trashes_then_purges() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let id = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    let keep = app.share(&s, "b", "public", &[("f:index.html", HTML)]).await;
    let now = OffsetDateTime::now_utc();
    sqlx::query("UPDATE shares SET expires_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now + Duration::hours(1))
        .execute(&app.st.db)
        .await
        .unwrap();

    lifecycle::run_job(&app.st, now).await;
    assert_eq!(state(&app, id).await.as_deref(), Some("active"));
    // A later run trashes it.
    lifecycle::run_job(&app.st, now + Duration::hours(2)).await;
    assert_eq!(state(&app, id).await.as_deref(), Some("trashed"));
    assert_eq!(app.get(&format!("{SHARE}/a/"), &[]).await.status, 410);

    lifecycle::run_job(&app.st, now + Duration::days(29)).await;
    assert_eq!(state(&app, id).await.as_deref(), Some("trashed"));
    lifecycle::run_job(&app.st, now + Duration::days(31)).await;
    assert_eq!(state(&app, id).await, None);
    assert_eq!(app.get(&format!("{SHARE}/a/"), &[]).await.status, 404);
    assert!(app.keys().await.iter().all(|k| !k.contains(&id.to_string())));
    assert_eq!(state(&app, keep).await.as_deref(), Some("active"));
}

#[tokio::test]
async fn owner_extends_before_the_job_writes() {
    let app = App::new().await;
    let (_, s) = app.user("alice").await;
    let id = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    let now = OffsetDateTime::now_utc();
    // Expiring soon; the owner sets "never" before the job's write lands.
    sqlx::query("UPDATE shares SET expires_at = $1")
        .bind(now + Duration::minutes(5))
        .execute(&app.st.db)
        .await
        .unwrap();
    let res = app
        .post_form(
            &format!("{ADMIN}/shares/{id}/edit"),
            &admin_cookie(&s),
            &[("path", "a"), ("visibility", "public"), ("expiry", "never")],
        )
        .await;
    assert_eq!(res.status, 303);
    lifecycle::run_job(&app.st, now + Duration::hours(1)).await;
    assert_eq!(state(&app, id).await.as_deref(), Some("active"));
    assert_eq!(app.get(&format!("{SHARE}/a/"), &[]).await.status, 200);

    // Already expired, but the job has not run yet: the owner can still extend it.
    sqlx::query("UPDATE shares SET expires_at = now() - interval '1 minute'").execute(&app.st.db).await.unwrap();
    let res = app
        .post_form(
            &format!("{ADMIN}/shares/{id}/edit"),
            &admin_cookie(&s),
            &[("path", "a"), ("visibility", "public"), ("expiry", "never")],
        )
        .await;
    assert_eq!(res.status, 303, "{}", res.text());
    assert_eq!(state(&app, id).await.as_deref(), Some("active"));
}

#[tokio::test]
async fn delete_reuse_and_restore() {
    let app = App::new().await;
    let (uid, s) = app.user("alice").await;
    let old = app.share(&s, "notes", "password", &[("f:index.html", b"old")]).await;
    let res = app.post_form(&format!("{ADMIN}/shares/{old}/delete"), &admin_cookie(&s), &[]).await;
    assert_eq!(res.status, 303);
    assert_eq!(app.get(&format!("{SHARE}/notes/"), &[]).await.status, 410);
    assert_eq!(app.keys().await.len(), 1, "delete keeps the files");
    let trash = app.get(&format!("{ADMIN}/shares?tab=trash"), &[("cookie", &admin_cookie(&s))]).await;
    assert!(trash.text().contains("/notes/") && trash.text().contains("30 days left"));

    // Path reuse, then restore with a taken path.
    app.share(&s, "notes", "public", &[("f:index.html", b"new")]).await;
    let res = app.post_form(&format!("{ADMIN}/shares/{old}/restore"), &admin_cookie(&s), &[]).await;
    assert_eq!(res.status, 200);
    assert!(res.text().contains("name=\"path\""), "asks for a new path");
    assert_eq!(state(&app, old).await.as_deref(), Some("trashed"));

    let res =
        app.post_form(&format!("{ADMIN}/shares/{old}/restore"), &admin_cookie(&s), &[("path", "notes-old")]).await;
    assert_eq!(res.status, 303);
    let row: (String, String, bool) =
        sqlx::query_as("SELECT path, visibility, password_hash IS NOT NULL FROM shares WHERE id = $1")
            .bind(old)
            .fetch_one(&app.st.db)
            .await
            .unwrap();
    assert_eq!(row, ("notes-old".into(), "password".into(), true));

    // A past expiry is cleared on restore.
    lifecycle::delete(&app.st, uid, old, OffsetDateTime::now_utc()).await.unwrap();
    sqlx::query("UPDATE shares SET expires_at = now() - interval '1 day' WHERE id = $1")
        .bind(old)
        .execute(&app.st.db)
        .await
        .unwrap();
    lifecycle::restore(&app.st, uid, old, None, OffsetDateTime::now_utc()).await.unwrap().unwrap();
    let e: Option<OffsetDateTime> = sqlx::query_scalar("SELECT expires_at FROM shares WHERE id = $1")
        .bind(old)
        .fetch_one(&app.st.db)
        .await
        .unwrap();
    assert_eq!(e, None);
}

#[tokio::test]
async fn restore_during_purge_is_refused() {
    let app = App::new().await;
    let (uid, s) = app.user("alice").await;
    let id = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    lifecycle::delete(&app.st, uid, id, OffsetDateTime::now_utc()).await.unwrap();
    sqlx::query("UPDATE shares SET state = 'purging'").execute(&app.st.db).await.unwrap();
    let r = lifecycle::restore(&app.st, uid, id, None, OffsetDateTime::now_utc()).await.unwrap();
    assert_eq!(r, Err(RestoreError::NotFound));
    let trash = app.get(&format!("{ADMIN}/shares?tab=trash"), &[("cookie", &admin_cookie(&s))]).await;
    assert!(trash.text().contains("Deleting") && !trash.text().contains("/restore"));
    lifecycle::run_job(&app.st, OffsetDateTime::now_utc()).await;
    assert_eq!(state(&app, id).await, None);
}

#[tokio::test]
async fn restore_races_an_upload() {
    let app = App::new().await;
    let (uid, s) = app.user("alice").await;
    let id = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    lifecycle::delete(&app.st, uid, id, OffsetDateTime::now_utc()).await.unwrap();
    let set = [("path", "a/b"), ("visibility", "public"), ("expiry", "never")];
    let (restored, uploaded) = tokio::join!(
        lifecycle::restore(&app.st, uid, id, None, OffsetDateTime::now_utc()),
        app.upload(&s, &set, &[("f:index.html", HTML)]),
    );
    assert_ne!(restored.unwrap().is_ok(), uploaded.status == 200, "exactly one succeeds");
}

#[tokio::test]
async fn manual_purge_and_storage_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = dropgo::store::from_url(&format!("file://{}", dir.path().display())).unwrap();
    let app = App::with(config(), store).await;
    let (uid, s) = app.user("alice").await;
    let a = app.share(&s, "a", "public", &[("f:index.html", HTML)]).await;
    let b = app.share(&s, "b", "public", &[("f:index.html", HTML)]).await;
    let c = app.share(&s, "c", "public", &[("f:index.html", HTML)]).await;
    let now = OffsetDateTime::now_utc();
    for id in [a, b] {
        lifecycle::delete(&app.st, uid, id, now - Duration::days(31)).await.unwrap();
    }
    sqlx::query("UPDATE shares SET expires_at = now() - interval '1 minute' WHERE id = $1")
        .bind(c)
        .execute(&app.st.db)
        .await
        .unwrap();

    // Deleting a's files fails; b is still purged and c is still trashed.
    use std::os::unix::fs::PermissionsExt;
    let locked = dir.path().join(format!("shares/{a}"));
    let upload = std::fs::read_dir(&locked).unwrap().next().unwrap().unwrap().path();
    std::fs::set_permissions(upload.join("files"), std::fs::Permissions::from_mode(0o555)).unwrap();
    lifecycle::run_job(&app.st, now).await;
    assert_eq!(state(&app, a).await.as_deref(), Some("purging"));
    assert_eq!(app.get(&format!("{SHARE}/a/"), &[]).await.status, 410);
    assert_eq!(lifecycle::restore(&app.st, uid, a, None, now).await.unwrap(), Err(RestoreError::NotFound));
    assert_eq!(state(&app, b).await, None);
    assert_eq!(state(&app, c).await.as_deref(), Some("trashed"));

    std::fs::set_permissions(upload.join("files"), std::fs::Permissions::from_mode(0o755)).unwrap();
    lifecycle::run_job(&app.st, now).await;
    assert_eq!(state(&app, a).await, None, "the next run retries");

    // Manual purge.
    let res = app.post_form(&format!("{ADMIN}/shares/{c}/purge"), &admin_cookie(&s), &[]).await;
    assert_eq!(res.status, 303);
    assert_eq!(state(&app, c).await, None);
    assert!(app.keys().await.is_empty());
}
