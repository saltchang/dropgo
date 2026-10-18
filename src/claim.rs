//! Claiming a share path. Every write that takes a path runs through `claim` under one lock.

use sqlx::{PgConnection, Row};
use time::OffsetDateTime;
use uuid::Uuid;

/// Any constant works; it only has to be the same for every claim.
const LOCK_KEY: i64 = 0x6472_6f70_676f;

#[derive(Debug, PartialEq)]
pub enum Claim {
    /// The path is free.
    Free,
    /// The owner confirmed overwriting this share; `upload_id` is the one being replaced.
    Overwrite { upload_id: Uuid },
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum ClaimError {
    /// Another live share equals, contains or sits under the path.
    #[error("conflict with {0}")]
    Conflict(String),
    /// The owner's own share is at this exact path and no overwrite was confirmed.
    #[error("overwrite needed for {0}")]
    NeedsOverwrite(Uuid),
}

const CONFLICTS: &str =
    "state = 'active' AND (path = $1 OR starts_with(path, $1 || '/') OR starts_with($1, path || '/'))";

/// Takes the claim lock, trashes expired shares in the way, then checks for conflicts.
/// `exclude` is a share that may keep its own path (a path change or a restore).
/// `overwrite` is the share the owner confirmed replacing.
pub async fn claim(
    tx: &mut PgConnection,
    owner: Uuid,
    path: &str,
    exclude: Option<Uuid>,
    overwrite: Option<Uuid>,
    now: OffsetDateTime,
) -> sqlx::Result<Result<Claim, ClaimError>> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(LOCK_KEY).execute(&mut *tx).await?;
    sqlx::query(&format!(
        "UPDATE shares SET state = 'trashed', trashed_at = $2 WHERE {CONFLICTS} AND expires_at <= $2 AND id IS DISTINCT FROM $3"
    ))
    .bind(path)
    .bind(now)
    .bind(exclude)
    .execute(&mut *tx)
    .await?;
    let rows = sqlx::query(&format!(
        "SELECT id, path, owner_id, upload_id FROM shares WHERE {CONFLICTS} AND id IS DISTINCT FROM $2 ORDER BY path"
    ))
    .bind(path)
    .bind(exclude)
    .fetch_all(&mut *tx)
    .await?;
    Ok(decide(&rows, owner, path, overwrite))
}

/// The same check without the lock, for failing early before an upload is sent.
pub async fn precheck(
    db: &sqlx::PgPool,
    owner: Uuid,
    path: &str,
    overwrite: Option<Uuid>,
) -> sqlx::Result<Result<Claim, ClaimError>> {
    let rows = sqlx::query(&format!(
        "SELECT id, path, owner_id, upload_id FROM shares WHERE {CONFLICTS} \
         AND (expires_at IS NULL OR expires_at > now()) ORDER BY path"
    ))
    .bind(path)
    .fetch_all(db)
    .await?;
    Ok(decide(&rows, owner, path, overwrite))
}

fn decide(
    rows: &[sqlx::postgres::PgRow],
    owner: Uuid,
    path: &str,
    overwrite: Option<Uuid>,
) -> Result<Claim, ClaimError> {
    let Some(row) = rows.first() else { return Ok(Claim::Free) };
    let (id, other_path, owner_id): (Uuid, String, Uuid) = (row.get(0), row.get(1), row.get(2));
    if rows.len() > 1 || other_path != path || owner_id != owner {
        return Err(ClaimError::Conflict(other_path));
    }
    match overwrite {
        Some(o) if o == id => Ok(Claim::Overwrite { upload_id: row.get(3) }),
        _ => Err(ClaimError::NeedsOverwrite(id)),
    }
}
