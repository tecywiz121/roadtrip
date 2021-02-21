use snafu::Snafu;

use std::path::{Path, PathBuf};

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum Error {
    ReadDir {
        source: std::io::Error,
        path: PathBuf,
    },
    Canonicalize {
        source: std::io::Error,
        path: PathBuf,
    },
    Metadata {
        source: std::io::Error,
        path: PathBuf,
    },
}

impl Error {
    pub fn path(&self) -> &Path {
        use self::Error::*;

        match self {
            ReadDir { path, .. } => &path,
            Canonicalize { path, .. } => &path,
            Metadata { path, .. } => &path,
        }
    }
}
