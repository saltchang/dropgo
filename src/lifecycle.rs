//! Expiry, trash, restore and purge. Every write re-checks its condition, so the job needs no lock.

use time::{Duration, OffsetDateTime};
use tower_sessions::ExpiredDeletion;
use uuid::Uuid;

use crate::{
    AppState,
    claim::{self, Claim, ClaimError},
    store,
};

pub const TRASH_DAYS: i64 = 30;

/// One run of the hourly job: trash expired shares, then purge old trash.
pub async fn run_job(st: &AppState, now: OffsetDateTime) {
    // One conditional statement: an owner who extends the expiry first wins.
    if let Err(e) =
        sqlx::query("UPDATE shares SET state = 'trashed', trashed_at = $1 WHERE state = 'active' AND expires_at <= $1")
            .bind(now)
            .execute(&st.db)
            .await
    {
        tracing::error!("trashing expired shares: {e}");
    }
    if let Err(e) = sqlx::query("UPDATE shares SET state = 'purging' WHERE state = 'trashed' AND trashed_at <= $1")
        .bind(now - Duration::days(TRASH_DAYS))
        .execute(&st.db)
        .await
    {
        tracing::error!("marking shares for purge: {e}");
    }
    // Includes rows left in `purging` by an earlier failed run.
    let ids: Vec<Uuid> =
        match sqlx::query_scalar("SELECT id FROM shares WHERE state = 'purging'").fetch_all(&st.db).await {
            Ok(ids) => ids,
            Err(e) => return tracing::error!("listing shares to purge: {e}"),
        };
    for id in ids {
        if let Err(e) = purge_files_and_row(st, id).await {
            tracing::error!("purging share {id}: {e:#}");
        }
    }
}

async fn purge_files_and_row(st: &AppState, id: Uuid) -> anyhow::Result<()> {
    store::delete_prefix(&st.store, &store::share_prefix(id)).await?;
    sqlx::query("DELETE FROM shares WHERE id = $1 AND state = 'purging'").bind(id).execute(&st.db).await?;
    Ok(())
}

pub async fn spawn_job(st: AppState) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        tick.tick().await;
        run_job(&st, OffsetDateTime::now_utc()).await;
        if let Err(e) = st.sessions.delete_expired().await {
            tracing::error!("deleting expired sessions: {e}");
        }
    }
}

/// Moves an active share to the trash. Returns false when there is no such share.
pub async fn delete(st: &AppState, owner: Uuid, id: Uuid, now: OffsetDateTime) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE shares SET state = 'trashed', trashed_at = $3 WHERE id = $1 AND owner_id = $2 AND state = 'active'",
    )
    .bind(id)
    .bind(owner)
    .bind(now)
    .execute(&st.db)
    .await?;
    Ok(r.rows_affected() == 1)
}

#[derive(Debug, PartialEq)]
pub enum RestoreError {
    /// Not in the trash: unknown, active, or already being purged.
    NotFound,
    /// The path is taken now; carries the conflicting path.
    Conflict(String),
}

/// Restores a trashed share at its old path or at `new_path`, through the same claim as uploads.
pub async fn restore(
    st: &AppState,
    owner: Uuid,
    id: Uuid,
    new_path: Option<&str>,
    now: OffsetDateTime,
) -> anyhow::Result<Result<(), RestoreError>> {
    let mut tx = st.db.begin().await?;
    // The final UPDATE re-checks `trashed`, so a purge that starts in between wins.
    let old: Option<String> =
        sqlx::query_scalar("SELECT path FROM shares WHERE id = $1 AND owner_id = $2 AND state = 'trashed'")
            .bind(id)
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(old) = old else { return Ok(Err(RestoreError::NotFound)) };
    let path = new_path.unwrap_or(&old);
    match claim::claim(&mut tx, owner, path, Some(id), None, now).await? {
        Ok(Claim::Free) => {}
        Ok(Claim::Overwrite { .. }) => unreachable!("no overwrite was asked for"),
        Err(ClaimError::Conflict(p)) => return Ok(Err(RestoreError::Conflict(p))),
        Err(ClaimError::NeedsOverwrite(_)) => return Ok(Err(RestoreError::Conflict(path.to_string()))),
    }
    let r = sqlx::query(
        "UPDATE shares SET state = 'active', trashed_at = NULL, path = $3, \
         expires_at = CASE WHEN expires_at <= $4 THEN NULL ELSE expires_at END \
         WHERE id = $1 AND owner_id = $2 AND state = 'trashed'",
    )
    .bind(id)
    .bind(owner)
    .bind(path)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    if r.rows_affected() != 1 {
        return Ok(Err(RestoreError::NotFound));
    }
    tx.commit().await?;
    Ok(Ok(()))
}

/// Purges a trashed share now. Returns false when it is not in the trash.
pub async fn purge(st: &AppState, owner: Uuid, id: Uuid) -> anyhow::Result<bool> {
    let r = sqlx::query("UPDATE shares SET state = 'purging' WHERE id = $1 AND owner_id = $2 AND state = 'trashed'")
        .bind(id)
        .bind(owner)
        .execute(&st.db)
        .await?;
    if r.rows_affected() != 1 {
        return Ok(false);
    }
    purge_files_and_row(st, id).await?;
    Ok(true)
}
