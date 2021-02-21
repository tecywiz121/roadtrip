mod util;

use roadtrip_cache::Cache;

use self::util::*;

use std::collections::HashMap;

use tempfile::tempdir;

use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn insert_two() -> Result<(), Error> {
    let root = tempdir()?;
    let cache = Cache::new(root.path(), 100).await?;

    {
        let entry = assert_vacant_entry(cache.entry("one").await?).await?;

        entry
            .insert_with("file0", |mut f| async move {
                f.write_all(b"hello earth").await?;
                Ok(())
            })
            .await?;

        entry
            .insert_with("file1", |mut f| async move {
                f.write_all(b"hello mars").await?;
                Ok(())
            })
            .await?;
    }

    {
        let mut expected: HashMap<_, &[u8]> = HashMap::new();
        expected.insert("file0", b"hello earth");
        expected.insert("file1", b"hello mars");
        assert_entry_eq(cache.entry("one").await?, expected).await?;
    }

    Ok(())
}

#[tokio::test]
async fn insert_one_at_capacity() -> Result<(), Error> {
    let root = tempdir()?;
    let cache = Cache::new(root.path(), 10).await?;

    {
        let entry = assert_vacant_entry(cache.entry("one").await?).await?;

        entry
            .insert_with("file0", |mut f| async move {
                f.write_all(b"1234567890").await?;
                Ok(())
            })
            .await?;
    }

    {
        let mut expected: HashMap<_, &[u8]> = HashMap::new();
        expected.insert("file0", b"1234567890");

        assert_entry_eq(cache.entry("one").await?, expected).await?;
    }

    Ok(())
}

#[tokio::test]
async fn insert_one_over_capacity() -> Result<(), Error> {
    let root = tempdir()?;
    let cache = Cache::new(root.path(), 1).await?;

    {
        let entry = assert_vacant_entry(cache.entry("one").await?).await?;

        entry
            .insert_with("file0", |mut f| async move {
                f.write_all(b"1234567890").await?;
                Ok(())
            })
            .await?;
    }

    {
        let mut expected: HashMap<_, &[u8]> = HashMap::new();
        expected.insert("file0", b"1234567890");

        assert_entry_eq(cache.entry("one").await?, expected).await?;
    }

    Ok(())
}

#[tokio::test]
async fn insert_many_over_capacity() -> Result<(), Error> {
    let root = tempdir()?;
    let cache = Cache::new(root.path(), 1).await?;

    {
        let entry = assert_vacant_entry(cache.entry("one").await?).await?;

        entry
            .insert_with("file0", |mut f| async move {
                f.write_all(b"1234567890").await?;
                Ok(())
            })
            .await?;

        entry
            .insert_with("file1", |mut f| async move {
                f.write_all(b"0987654321").await?;
                Ok(())
            })
            .await?;
    }

    {
        let mut expected: HashMap<_, &[u8]> = HashMap::new();
        expected.insert("file0", b"1234567890");
        expected.insert("file1", b"0987654321");

        assert_entry_eq(cache.entry("one").await?, expected).await?;
    }

    Ok(())
}

#[tokio::test]
async fn insert_evict() -> Result<(), Error> {
    let root = tempdir()?;
    let cache = Cache::new(root.path(), 1).await?;

    {
        let entry = assert_vacant_entry(cache.entry("one").await?).await?;

        entry
            .insert_with("file0", |mut f| async move {
                f.write_all(b"1234567890").await?;
                Ok(())
            })
            .await?;
    }

    {
        let entry = assert_vacant_entry(cache.entry("two").await?).await?;

        entry
            .insert_with("file1", |mut f| async move {
                f.write_all(b"0987654321").await?;
                Ok(())
            })
            .await?;
    }

    {
        assert_vacant_entry(cache.entry("one").await?).await?;

        let mut expected: HashMap<_, &[u8]> = HashMap::new();
        expected.insert("file1", b"0987654321");

        assert_entry_eq(cache.entry("two").await?, expected).await?;
    }

    Ok(())
}

#[tokio::test]
async fn insert_evict_multiple() -> Result<(), Error> {
    let root = tempdir()?;
    let cache = Cache::new(root.path(), 10).await?;

    for idx in 0..10 {
        let key = format!("entry{}", idx);
        let entry = assert_vacant_entry(cache.entry(&key).await?).await?;

        entry
            .insert_with("file0", |mut f| async move {
                f.write_all(b"0").await?;
                Ok(())
            })
            .await?;
    }

    assert_eq(10, cache.len().await)?;
    assert_eq(10, cache.size().await)?;

    {
        let entry = assert_vacant_entry(cache.entry("two").await?).await?;

        entry
            .insert_with("file1", |mut f| async move {
                f.write_all(b"21").await?;
                Ok(())
            })
            .await?;
    }

    assert_eq(9, cache.len().await)?;
    assert_eq(10, cache.size().await)?;

    {
        assert_vacant_entry(cache.entry("entry0").await?).await?;
        assert_vacant_entry(cache.entry("entry1").await?).await?;

        let mut byte: HashMap<_, &[u8]> = HashMap::new();
        byte.insert("file0", b"0");

        for idx in 2..10 {
            let key = format!("entry{}", idx);
            assert_entry_eq(cache.entry(&key).await?, byte.clone()).await?;
        }

        let mut expected: HashMap<_, &[u8]> = HashMap::new();
        expected.insert("file1", b"21");

        assert_entry_eq(cache.entry("two").await?, expected.clone()).await?;
    }

    Ok(())
}

#[tokio::test]
async fn insert_evict_multiple_parts() -> Result<(), Error> {
    let root = tempdir()?;
    let cache = Cache::new(root.path(), 10).await?;

    for idx in 0..9 {
        let key = format!("entry{}", idx);
        let entry = assert_vacant_entry(cache.entry(&key).await?).await?;

        entry
            .insert_with("file0", |mut f| async move {
                f.write_all(b"0").await?;
                Ok(())
            })
            .await?;
    }

    assert_eq(9, cache.len().await)?;
    assert_eq(9, cache.size().await)?;

    {
        let entry = assert_vacant_entry(cache.entry("two").await?).await?;

        entry
            .insert_with("file1", |mut f| async move {
                f.write_all(b"1").await?;
                Ok(())
            })
            .await?;

        entry
            .insert_with("file2", |mut f| async move {
                f.write_all(b"g").await?;
                Ok(())
            })
            .await?;
    }

    assert_eq(9, cache.len().await)?;
    assert_eq(10, cache.size().await)?;

    {
        assert_vacant_entry(cache.entry("entry0").await?).await?;

        let mut byte: HashMap<_, &[u8]> = HashMap::new();
        byte.insert("file0", b"0");

        for idx in 1..9 {
            let key = format!("entry{}", idx);
            assert_entry_eq(cache.entry(&key).await?, byte.clone()).await?;
        }

        let mut expected: HashMap<_, &[u8]> = HashMap::new();
        expected.insert("file1", b"1");
        expected.insert("file2", b"g");

        assert_entry_eq(cache.entry("two").await?, expected.clone()).await?;
    }

    Ok(())
}

#[tokio::test]
async fn lock() -> Result<(), Error> {
    let root = tempdir()?;
    let cache0 = Cache::new(root.path(), 10).await?;

    match Cache::new(root.path(), 10).await {
        Err(roadtrip_cache::error::Error::AlreadyLocked) => (),
        _ => return Error::other("cache dir should have been locked"),
    }

    drop(cache0);

    Ok(())
}
