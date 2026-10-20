//! Uploads: receive into a temp dir, then a tracked task stores the files and claims the path.

use std::io::{Read, Write};
use std::path::{Path as FsPath, PathBuf};

use axum::{
    Extension, Json,
    extract::{Multipart, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::TryStreamExt;
use object_store::{ObjectStoreExt, PutPayload};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    AppState,
    auth::OwnerId,
    claim::{self, Claim, ClaimError},
    config::MAX_FILES,
    i18n::T,
    ignore_rules, paths, store,
};

/// A user-facing failure: HTTP status, catalog key, and details for the overwrite prompt.
#[derive(Debug, Clone)]
pub struct Fail {
    pub status: StatusCode,
    pub key: &'static str,
    pub arg: Option<String>,
    pub existing: Option<Existing>,
}

impl Fail {
    pub fn new(status: StatusCode, key: &'static str) -> Self {
        Fail { status, key, arg: None, existing: None }
    }
    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.arg = Some(arg.into());
        self
    }
    pub fn message(&self, t: T<'_>) -> String {
        t.s(self.key).replace("{}", self.arg.as_deref().unwrap_or_default())
    }
    pub fn into_response(self, t: T<'_>) -> Response {
        (self.status, Json(json!({ "error": self.message(t), "existing": self.existing }))).into_response()
    }
}

pub fn internal(e: impl std::fmt::Display) -> Fail {
    tracing::error!("upload: {e}");
    Fail::new(StatusCode::INTERNAL_SERVER_ERROR, "upload.internal")
}

/// The owner's share at a path, so the form can offer to overwrite it with its settings filled in.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Existing {
    pub id: Uuid,
    pub visibility: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
}

/// Turns a claim conflict into the message the owner sees.
pub async fn claim_fail(db: &sqlx::PgPool, e: ClaimError) -> Fail {
    match e {
        ClaimError::Conflict(p) => Fail::new(StatusCode::CONFLICT, "path.conflict").with_arg(p),
        ClaimError::NeedsOverwrite(id) => {
            let existing = sqlx::query_as("SELECT id, visibility, expires_at FROM shares WHERE id = $1")
                .bind(id)
                .fetch_optional(db)
                .await
                .ok()
                .flatten();
            Fail { existing, ..Fail::new(StatusCode::CONFLICT, "path.exists") }
        }
    }
}

pub fn path_fail(e: paths::PathError) -> Fail {
    Fail::new(
        StatusCode::BAD_REQUEST,
        match e {
            paths::PathError::TooManySegments => "path.too_many_segments",
            paths::PathError::BadSegment => "path.bad_segment",
            paths::PathError::Reserved => "path.reserved",
        },
    )
}

pub fn parse_visibility(v: &str) -> Result<&'static str, Fail> {
    ["public", "password", "owner"]
        .into_iter()
        .find(|x| *x == v)
        .ok_or_else(|| Fail::new(StatusCode::BAD_REQUEST, "form.bad_visibility"))
}

/// `never`, `1d`, `7d`, `30d`, or `custom` with an RFC 3339 time from the browser.
pub fn parse_expiry(choice: &str, custom: &str, now: OffsetDateTime) -> Result<Option<OffsetDateTime>, Fail> {
    let days = |d| Ok(Some(now + Duration::days(d)));
    match choice {
        "" | "never" => Ok(None),
        "1d" => days(1),
        "7d" => days(7),
        "30d" => days(30),
        "custom" => OffsetDateTime::parse(custom, &Rfc3339)
            .ok()
            .filter(|t| *t > now)
            .map(Some)
            .ok_or_else(|| Fail::new(StatusCode::BAD_REQUEST, "form.bad_expiry")),
        _ => Err(Fail::new(StatusCode::BAD_REQUEST, "form.bad_expiry")),
    }
}

pub async fn owner_t(st: &AppState, owner: Uuid) -> T<'_> {
    let locale: String = sqlx::query_scalar("SELECT locale FROM users WHERE id = $1")
        .bind(owner)
        .fetch_optional(&st.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    st.i18n.t(&locale)
}

#[derive(Deserialize)]
pub struct CheckQuery {
    pub path: String,
}

/// The normalized path when it is free, `None` for an empty path, which gets a random one.
pub async fn check(st: &AppState, owner: Uuid, raw: &str) -> Result<Option<String>, Fail> {
    let Some(path) = paths::parse_share_path(raw).map_err(path_fail)? else { return Ok(None) };
    match claim::precheck(&st.db, owner, &path, None).await.map_err(internal)? {
        Ok(_) => Ok(Some(path)),
        Err(e) => Err(claim_fail(&st.db, e).await),
    }
}

/// Lets the upload form show conflicts and the overwrite prompt while the owner types.
pub async fn check_path(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    Query(q): Query<CheckQuery>,
) -> Response {
    match check(&st, owner, &q.path).await {
        Ok(None) => Json(json!({ "ok": true })).into_response(),
        Ok(Some(path)) => Json(json!({ "ok": true, "path": path })).into_response(),
        Err(f) => f.into_response(owner_t(&st, owner).await),
    }
}

#[derive(Default)]
struct Settings {
    path: String,
    visibility: String,
    password: String,
    expiry: String,
    expires_at: String,
    overwrite: String,
}

/// Everything the upload task needs once the request body has been received.
pub struct Job {
    pub owner: Uuid,
    pub share_id: Uuid,
    pub upload_id: Uuid,
    pub path: String,
    pub visibility: &'static str,
    pub password: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub overwrite: Option<Uuid>,
    /// (file on disk, relative path in the share)
    pub files: Vec<(PathBuf, String)>,
    pub entry: String,
    pub size: u64,
    /// Deleted when the job is dropped.
    pub _tmp: tempfile::TempDir,
}

pub async fn upload(
    State(st): State<AppState>,
    Extension(OwnerId(owner)): Extension<OwnerId>,
    multipart: Multipart,
) -> Response {
    match receive_and_store(&st, owner, multipart).await {
        Ok((_, path)) => {
            Json(json!({ "url": format!("{}/{path}/", st.cfg.share_origin), "path": path })).into_response()
        }
        Err(f) => f.into_response(owner_t(&st, owner).await),
    }
}

/// The whole upload, for the Web UI and the API. Returns the share id and its path.
pub async fn receive_and_store(st: &AppState, owner: Uuid, multipart: Multipart) -> Result<(Uuid, String), Fail> {
    let job = receive(st, owner, multipart).await?;
    let id = job.share_id;
    // A spawned task keeps going if the client disconnects now, and shutdown waits for it.
    let path = st.tracker.spawn(run(st.clone(), job)).await.map_err(internal)??;
    Ok((id, path))
}

async fn receive(st: &AppState, owner: Uuid, mut mp: Multipart) -> Result<Job, Fail> {
    let max = st.cfg.max_share_bytes;
    let too_big = || Fail::new(StatusCode::PAYLOAD_TOO_LARGE, "upload.too_big");
    let bad_path = |p: &str| Fail::new(StatusCode::BAD_REQUEST, "upload.bad_file_path").with_arg(p);
    let tmp = tempfile::Builder::new().prefix("dropgo-").tempdir_in(st.cfg.tmp_dir()).map_err(internal)?;
    let root = tmp.path().join("files");
    let mut s = Settings::default();
    let mut checked: Option<(String, &'static str, Option<Uuid>)> = None;
    let mut total = 0u64;
    let mut count = 0usize;
    let mut zip: Option<PathBuf> = None;

    while let Some(mut field) =
        mp.next_field().await.map_err(|_| Fail::new(StatusCode::BAD_REQUEST, "upload.interrupted"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        let is_file = name.starts_with("f:") || name == "zip";
        if !is_file {
            // The precheck ran on the first file, so a later setting would be silently ignored.
            if checked.is_some() {
                return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.field_order"));
            }
            let v = field.text().await.map_err(|_| Fail::new(StatusCode::BAD_REQUEST, "upload.interrupted"))?;
            match name.as_str() {
                "path" => s.path = v,
                "visibility" => s.visibility = v,
                "password" => s.password = v,
                "expiry" => s.expiry = v,
                "expires_at" => s.expires_at = v,
                "overwrite" => s.overwrite = v,
                _ => {}
            }
            continue;
        }
        // Settings come first, so an obvious conflict fails before the files are sent.
        if checked.is_none() {
            checked = Some(precheck(st, owner, &s).await?);
        }
        let dest = if name == "zip" {
            if zip.is_some() || count > 0 {
                return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.mixed"));
            }
            let p = tmp.path().join("upload.zip");
            zip = Some(p.clone());
            p
        } else {
            let rel = &name[2..];
            if !paths::valid_rel_path(rel) {
                return Err(bad_path(rel));
            }
            count += 1;
            if count > MAX_FILES || zip.is_some() {
                return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.too_many_files"));
            }
            root.join(rel)
        };
        if let Some(dir) = dest.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(internal)?;
        }
        let mut out = tokio::fs::File::create(&dest).await.map_err(internal)?;
        while let Some(chunk) =
            field.chunk().await.map_err(|_| Fail::new(StatusCode::BAD_REQUEST, "upload.interrupted"))?
        {
            total += chunk.len() as u64;
            if total > max {
                return Err(too_big());
            }
            out.write_all(&chunk).await.map_err(internal)?;
        }
        out.flush().await.map_err(internal)?;
    }
    let Some((path, visibility, overwrite)) = checked else {
        return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.empty"));
    };

    let root = match zip {
        Some(z) => {
            let root = root.clone();
            tokio::task::spawn_blocking(move || unzip(&z, &root, max).map(|()| zip_root(&root)))
                .await
                .map_err(internal)??
        }
        None => root,
    };
    let kept = if root.exists() { ignore_rules::kept_files(&root).map_err(internal)? } else { vec![] };
    if kept.is_empty() {
        return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.empty"));
    }
    let strip = ignore_rules::single_top_dir(&kept);
    let rel = |f: &String| strip.as_ref().map_or(f.clone(), |d| f[d.len() + 1..].to_string());
    let rels: Vec<String> = kept.iter().map(rel).collect();
    let entry = ignore_rules::entry_file(&rels).ok_or_else(|| Fail::new(StatusCode::BAD_REQUEST, "upload.no_entry"))?;
    let mut size = 0;
    let mut files = Vec::with_capacity(kept.len());
    for (k, r) in kept.iter().zip(rels) {
        let p = root.join(k);
        size += std::fs::metadata(&p).map_err(internal)?.len();
        files.push((p, r));
    }

    let now = OffsetDateTime::now_utc();
    Ok(Job {
        owner,
        share_id: overwrite.unwrap_or_else(Uuid::new_v4),
        upload_id: Uuid::new_v4(),
        path,
        visibility,
        password: Some(s.password).filter(|p| !p.is_empty()),
        expires_at: parse_expiry(&s.expiry, &s.expires_at, now)?,
        overwrite,
        files,
        entry,
        size,
        _tmp: tmp,
    })
}

/// Validates the settings and checks the path without the lock.
async fn precheck(st: &AppState, owner: Uuid, s: &Settings) -> Result<(String, &'static str, Option<Uuid>), Fail> {
    let path = paths::parse_share_path(&s.path).map_err(path_fail)?.unwrap_or_else(paths::random_share_path);
    let visibility = parse_visibility(&s.visibility)?;
    parse_expiry(&s.expiry, &s.expires_at, OffsetDateTime::now_utc())?;
    let overwrite = if s.overwrite.is_empty() {
        None
    } else {
        Some(s.overwrite.parse::<Uuid>().map_err(|_| Fail::new(StatusCode::BAD_REQUEST, "form.bad_overwrite"))?)
    };
    if visibility == "password" && s.password.is_empty() && overwrite.is_none() {
        return Err(Fail::new(StatusCode::BAD_REQUEST, "form.password_required"));
    }
    match claim::precheck(&st.db, owner, &path, overwrite).await.map_err(internal)? {
        Ok(_) => Ok((path, visibility, overwrite)),
        Err(e) => Err(claim_fail(&st.db, e).await),
    }
}

/// A zip of a folder holds the folder itself. Use it as the root, like a picked folder,
/// so its `.dockerignore` counts as top level.
fn zip_root(root: &FsPath) -> PathBuf {
    // Finder junk is not a second top-level entry. Top-level ignore files are, because their rules apply from here.
    let entries: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name == ".gitignore" || name == ".dockerignore" || (name != "__MACOSX" && !name.starts_with('.'))
        })
        .collect();
    match entries.as_slice() {
        [only] if only.path().is_dir() => only.path(),
        _ => root.to_path_buf(),
    }
}

/// Unpacks a zip under `root`, refusing unsafe paths and stopping once `max` bytes are written.
fn unzip(zip_path: &FsPath, root: &FsPath, max: u64) -> Result<(), Fail> {
    let bad = || Fail::new(StatusCode::BAD_REQUEST, "upload.bad_zip");
    let file = std::fs::File::open(zip_path).map_err(internal)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| bad())?;
    if archive.len() > MAX_FILES * 2 {
        return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.too_many_files"));
    }
    let mut total = 0u64;
    let mut count = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|_| bad())?;
        let name = entry.name().map_err(|_| bad())?.to_string();
        if entry.is_dir() {
            if !paths::valid_rel_path(name.trim_end_matches('/')) {
                return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.bad_file_path").with_arg(name));
            }
            continue;
        }
        if !paths::valid_rel_path(&name) {
            return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.bad_file_path").with_arg(name));
        }
        count += 1;
        if count > MAX_FILES {
            return Err(Fail::new(StatusCode::BAD_REQUEST, "upload.too_many_files"));
        }
        let dest = root.join(&name);
        std::fs::create_dir_all(dest.parent().expect("has parent")).map_err(internal)?;
        let mut out = std::fs::File::create(&dest).map_err(internal)?;
        // Count what is actually written; the sizes in the zip header can lie.
        let mut limited = (&mut entry).take(max - total + 1);
        let n = std::io::copy(&mut limited, &mut out).map_err(|_| bad())?;
        total += n;
        if total > max {
            return Err(Fail::new(StatusCode::PAYLOAD_TOO_LARGE, "upload.too_big"));
        }
        out.flush().map_err(internal)?;
    }
    Ok(())
}

/// Stores the files, renders Markdown, then claims the path. Cleans up its own files on failure.
pub async fn run(st: AppState, job: Job) -> Result<String, Fail> {
    let prefix = store::upload_prefix(job.share_id, job.upload_id);
    let result = store_and_claim(&st, &job).await;
    if result.is_err()
        && let Err(e) = store::delete_prefix(&st.store, &prefix).await
    {
        tracing::error!("cleanup of {prefix} failed: {e}");
    }
    result
}

async fn store_and_claim(st: &AppState, job: &Job) -> Result<String, Fail> {
    futures::stream::iter(job.files.iter().map(Ok))
        .try_for_each_concurrent(8, |(src, rel)| async move {
            let bytes = tokio::fs::read(src).await.map_err(internal)?;
            if rel.ends_with(".md") {
                let name = rel.rsplit('/').next().unwrap_or(rel);
                let page = st.renderer.page(&String::from_utf8_lossy(&bytes), name);
                st.store
                    .put(&store::rendered_key(job.share_id, job.upload_id, rel), PutPayload::from(page))
                    .await
                    .map_err(internal)?;
            }
            st.store
                .put(&store::file_key(job.share_id, job.upload_id, rel), PutPayload::from(bytes))
                .await
                .map_err(internal)?;
            Ok(())
        })
        .await?;

    let hash = match &job.password {
        Some(p) if job.visibility == "password" => Some(crate::share::hash_password(p.clone()).await),
        _ => None,
    };
    let now = OffsetDateTime::now_utc();
    let mut tx = st.db.begin().await.map_err(internal)?;
    let claimed = claim::claim(&mut tx, job.owner, &job.path, None, job.overwrite, now).await.map_err(internal)?;
    let replaced = match claimed {
        Err(e) => return Err(claim_fail(&st.db, e).await),
        // The share to overwrite is gone, and our files sit under its id.
        Ok(Claim::Free) if job.overwrite.is_some() => return Err(Fail::new(StatusCode::CONFLICT, "upload.changed")),
        Ok(Claim::Free) => {
            sqlx::query(
                "INSERT INTO shares (id, owner_id, path, state, visibility, password_hash, upload_id, entry, file_count, size_bytes, expires_at) \
                 VALUES ($1, $2, $3, 'active', $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(job.share_id)
            .bind(job.owner)
            .bind(&job.path)
            .bind(job.visibility)
            .bind(&hash)
            .bind(job.upload_id)
            .bind(&job.entry)
            .bind(job.files.len() as i32)
            .bind(job.size as i64)
            .bind(job.expires_at)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
            None
        }
        Ok(Claim::Overwrite { upload_id }) => {
            let has_password: bool = sqlx::query_scalar(
                "UPDATE shares SET upload_id = $2, entry = $3, file_count = $4, size_bytes = $5, visibility = $6, \
                 password_hash = CASE WHEN $6 = 'password' THEN COALESCE($7, password_hash) END, expires_at = $8 \
                 WHERE id = $1 RETURNING password_hash IS NOT NULL OR visibility <> 'password'",
            )
            .bind(job.share_id)
            .bind(job.upload_id)
            .bind(&job.entry)
            .bind(job.files.len() as i32)
            .bind(job.size as i64)
            .bind(job.visibility)
            .bind(&hash)
            .bind(job.expires_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
            if !has_password {
                return Err(Fail::new(StatusCode::BAD_REQUEST, "form.password_required"));
            }
            Some(upload_id)
        }
    };
    tx.commit().await.map_err(internal)?;
    if let Some(old) = replaced {
        // shortcut: a failed delete leaves orphans under the share prefix until purge.
        if let Err(e) = store::delete_prefix(&st.store, &store::upload_prefix(job.share_id, old)).await {
            tracing::error!("deleting replaced upload {old}: {e}");
        }
    }
    Ok(job.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_choices() {
        let now = OffsetDateTime::parse("2026-10-01T00:00:00Z", &Rfc3339).unwrap();
        assert_eq!(parse_expiry("never", "", now).unwrap(), None);
        assert_eq!(parse_expiry("7d", "", now).unwrap(), Some(now + Duration::days(7)));
        let custom = parse_expiry("custom", "2026-10-05T18:00:00+08:00", now).unwrap().unwrap();
        assert_eq!(custom, OffsetDateTime::parse("2026-10-05T10:00:00Z", &Rfc3339).unwrap());
        assert!(parse_expiry("custom", "2026-09-01T00:00:00Z", now).is_err(), "past");
        assert!(parse_expiry("custom", "tomorrow", now).is_err());
        assert!(parse_expiry("2d", "", now).is_err());
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let f = tempfile::NamedTempFile::new().unwrap();
        let mut z = zip::ZipWriter::new(f.reopen().unwrap());
        for (name, data) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        f
    }

    #[test]
    fn zip_root_skips_finder_junk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("site")).unwrap();
        std::fs::write(dir.path().join(".DS_Store"), "x").unwrap();
        assert_eq!(zip_root(dir.path()), dir.path().join("site"));
        std::fs::write(dir.path().join(".gitignore"), "*.log").unwrap();
        assert_eq!(zip_root(dir.path()), dir.path(), "a top-level .gitignore keeps the top level as root");
        std::fs::remove_file(dir.path().join(".gitignore")).unwrap();
        std::fs::write(dir.path().join("index.html"), "x").unwrap();
        assert_eq!(zip_root(dir.path()), dir.path());
    }

    #[test]
    fn unzip_rejects_slip_and_bombs() {
        let dir = tempfile::tempdir().unwrap();
        let slip = zip_of(&[("index.html", b"x"), ("../../etc/passwd", b"x")]);
        assert_eq!(unzip(slip.path(), dir.path(), 1000).unwrap_err().key, "upload.bad_file_path");

        let bomb = zip_of(&[("index.html", &vec![0u8; 5000])]);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unzip(bomb.path(), dir.path(), 1000).unwrap_err().key, "upload.too_big");

        let ok = zip_of(&[("site/index.html", b"<p>"), ("site/img/a.png", b"png")]);
        let dir = tempfile::tempdir().unwrap();
        unzip(ok.path(), dir.path(), 1000).unwrap();
        assert!(dir.path().join("site/img/a.png").exists());
    }
}
