use std::sync::Arc;

use anyhow::Context;
use futures::{StreamExt, TryStreamExt};
use object_store::{
    ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, local::LocalFileSystem, path::Path, prefix::PrefixStore,
};
use url::Url;

pub type Store = Arc<dyn ObjectStore>;

/// Builds the object store from `s3://bucket/prefix`, `file:///dir` or `memory://`.
/// S3 credentials and endpoint come from the usual `AWS_*` env vars.
pub fn from_url(raw: &str) -> anyhow::Result<Store> {
    let url = Url::parse(raw).context("DROPGO_STORE_URL")?;
    let (store, prefix): (Box<dyn ObjectStore>, Path) = match url.scheme() {
        "s3" => (Box::new(AmazonS3Builder::from_env().with_url(raw).build()?), Path::from(url.path())),
        "file" => {
            std::fs::create_dir_all(url.path()).context("create store directory")?;
            (Box::new(LocalFileSystem::new_with_prefix(url.path())?.with_automatic_cleanup(true)), Path::default())
        }
        _ => object_store::parse_url(&url)?,
    };
    Ok(if prefix.as_ref().is_empty() { Arc::from(store) } else { Arc::new(PrefixStore::new(store, prefix)) })
}

pub fn share_prefix(share_id: uuid::Uuid) -> Path {
    Path::from(format!("shares/{share_id}"))
}

pub fn upload_prefix(share_id: uuid::Uuid, upload_id: uuid::Uuid) -> Path {
    Path::from(format!("shares/{share_id}/{upload_id}"))
}

pub fn file_key(share_id: uuid::Uuid, upload_id: uuid::Uuid, rel: &str) -> Path {
    Path::from(format!("shares/{share_id}/{upload_id}/files/{rel}"))
}

pub fn rendered_key(share_id: uuid::Uuid, upload_id: uuid::Uuid, rel: &str) -> Path {
    Path::from(format!("shares/{share_id}/{upload_id}/rendered/{rel}.html"))
}

/// Deletes every object under `prefix`.
pub async fn delete_prefix(store: &Store, prefix: &Path) -> object_store::Result<()> {
    let locations = store.list(Some(prefix)).map_ok(|m| m.location).boxed();
    store.delete_stream(locations).try_collect::<Vec<_>>().await?;
    Ok(())
}

/// Relative paths and sizes of the original files of one upload, sorted.
pub async fn list_files(
    store: &Store,
    share_id: uuid::Uuid,
    upload_id: uuid::Uuid,
) -> object_store::Result<Vec<(String, u64)>> {
    let base = format!("shares/{share_id}/{upload_id}/files/");
    let mut out: Vec<(String, u64)> = store
        .list(Some(&Path::from(base.as_str())))
        // Keys are percent-encoded, so `100%.png` lists as `100%25.png`.
        .map_ok(|m| {
            let rel = m.location.as_ref().strip_prefix(&base).unwrap_or_default();
            (percent_encoding::percent_decode_str(rel).decode_utf8_lossy().into_owned(), m.size)
        })
        .try_collect()
        .await?;
    out.sort();
    Ok(out)
}

pub async fn get_bytes(store: &Store, key: &Path) -> object_store::Result<bytes::Bytes> {
    store.get(key).await?.bytes().await
}
