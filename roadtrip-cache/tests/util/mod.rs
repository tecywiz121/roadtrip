#![allow(unused)]

use roadtrip_cache::{Entry, VacantEntry};

use snafu::{OptionExt, Snafu};

use std::collections::HashMap;

use tokio::io::AsyncReadExt;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(context(false))]
    Io {
        source: std::io::Error,
    },
    #[snafu(context(false))]
    Cache {
        source: roadtrip_cache::error::Error,
    },
    #[snafu(context(false))]
    CacheEntry {
        source: roadtrip_cache::error::EntryError,
    },
    #[snafu(context(false))]
    CacheInsert {
        source: roadtrip_cache::error::InsertError,
    },

    Missing,

    Other {
        msg: String,
    },
}

impl Error {
    pub fn other<S, V>(s: S) -> Result<V, Error>
    where
        S: Into<String>,
    {
        Err(Error::Other { msg: s.into() })
    }
}

pub fn assert_eq<A, B>(a: A, b: B) -> Result<(), Error>
where
    A: Eq + PartialEq<B> + std::fmt::Debug,
    B: std::fmt::Debug,
{
    if a == b {
        Ok(())
    } else {
        let msg = format!("`{:?}` != `{:?}`", a, b);
        Err(Error::Other { msg })
    }
}

pub async fn assert_vacant_entry<'a>(
    entry: Entry<'a>,
) -> Result<VacantEntry<'a>, Error> {
    match entry {
        Entry::Vacant(v) => Ok(v),
        _ => Error::other("expected vacant entry"),
    }
}

pub async fn assert_entry_eq<'a>(
    entry: Entry<'a>,
    mut expected: HashMap<&'a str, &'a [u8]>,
) -> Result<(), Error> {
    let occupied = match entry {
        Entry::Occupied(o) => o,
        _ => return Error::other("expected occupied entry"),
    };

    for mut named_file in occupied.into_files() {
        let expected_contents =
            expected.remove(named_file.name()).context(Missing)?;
        let mut actual_contents = Vec::new();
        named_file.read_to_end(&mut actual_contents).await?;
        assert_eq(expected_contents, actual_contents)?;
    }

    assert_eq(expected.len(), 0)?;

    Ok(())
}

pub struct MapBuilder<'a>(HashMap<&'a str, &'a [u8]>);

impl<'a> MapBuilder<'a> {
    pub fn new() -> Self {
        MapBuilder(HashMap::new())
    }

    pub fn insert(mut self, key: &'a str, value: &'a [u8]) -> Self {
        self.0.insert(key, value);
        self
    }

    pub fn build(self) -> HashMap<&'a str, &'a [u8]> {
        self.0
    }
}
