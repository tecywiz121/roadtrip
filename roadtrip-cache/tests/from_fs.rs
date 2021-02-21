mod util;

use filetime::FileTime;

use roadtrip_cache::Cache;

use self::util::*;

use tempfile::{tempdir, TempDir};

use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;

async fn populate() -> Result<TempDir, Error> {
    let dir = tempdir()?;

    for path in &["entry0", "entry1", "entry2"] {
        fs::create_dir(dir.path().join(path)).await?;
    }

    let mut count = 0;
    for path in &["entry0/f0", "entry0/f1", "entry1/f2", "entry2/f3"] {
        count += 1;

        let mut file = File::create(dir.path().join(path)).await?;
        file.write_all(b"hello world").await?;

        // TODO: Asyncify setting file times.
        let stdfile = file.into_std().await;
        let ft = FileTime::from_unix_time(count, 0);
        filetime::set_file_handle_times(&stdfile, Some(ft), Some(ft))?;
    }

    Ok(dir)
}

#[tokio::test]
async fn read_from_fs() -> Result<(), Error> {
    let dir = populate().await?;

    let cache = Cache::new(dir.path(), 50).await?;

    let entry0 = MapBuilder::new()
        .insert("f0", b"hello world")
        .insert("f1", b"hello world")
        .build();

    assert_entry_eq(cache.entry("entry0").await?, entry0).await?;

    let entry1 = MapBuilder::new().insert("f2", b"hello world").build();

    assert_entry_eq(cache.entry("entry1").await?, entry1).await?;

    let entry2 = MapBuilder::new().insert("f3", b"hello world").build();

    assert_entry_eq(cache.entry("entry2").await?, entry2).await?;
    Ok(())
}

#[tokio::test]
async fn evict_one() -> Result<(), Error> {
    let dir = populate().await?;

    let cache = Cache::new(dir.path(), 50).await?;
    assert_eq(cache.len().await, 3)?;

    let entry3 = assert_vacant_entry(cache.entry("entry3").await?).await?;

    entry3
        .insert_with("f4", |mut f| async move {
            f.write_all(b"goodbye world").await?;
            Ok(())
        })
        .await?;

    assert_eq(cache.len().await, 3)?;
    assert_eq(cache.size().await, 35)?;

    assert_vacant_entry(cache.entry("entry0").await?).await?;

    let entry1 = MapBuilder::new().insert("f2", b"hello world").build();

    assert_entry_eq(cache.entry("entry1").await?, entry1).await?;

    let entry2 = MapBuilder::new().insert("f3", b"hello world").build();

    assert_entry_eq(cache.entry("entry2").await?, entry2).await?;

    let entry3 = MapBuilder::new().insert("f4", b"goodbye world").build();

    assert_entry_eq(cache.entry("entry3").await?, entry3).await?;
    Ok(())
}
