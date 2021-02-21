use snafu::Snafu;

use std::path::PathBuf;

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum EntryError {
    InvalidKey,
    ReadDir {
        path: PathBuf,
        source: std::io::Error,
    },
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    FileTime {
        source: std::io::Error,
    },
    Join {
        source: tokio::task::JoinError,
    },
    Prefix {
        source: std::path::StripPrefixError,
    },
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum InsertError {
    InvalidName,
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },
    Reopen {
        path: PathBuf,
        source: std::io::Error,
    },
    Reserve {
        source: std::io::Error,
    },
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum Error {
    #[snafu(context(false))]
    WalkDir {
        source: roadtrip_walkdir::error::Error,
    },
    Canonicalize {
        source: std::io::Error,
    },
    Structure {
        path: PathBuf,
    },
    Size {
        source: std::io::Error,
        path: PathBuf,
    },
    LockJoin {
        source: tokio::task::JoinError,
    },
    Lock {
        source: crate::lock::Error,
    },
    AlreadyLocked,
}
