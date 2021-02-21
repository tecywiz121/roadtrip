#![feature(map_first_last)]

pub mod error;

use crate::error::Error;

use futures::Stream;

pub use snafu;
use snafu::ResultExt;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::Metadata;
use std::path::{Path, PathBuf};

use tokio::fs;

#[derive(Debug, Clone, Copy)]
pub struct FileType {
    is_dir: bool,
}

impl FileType {
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
}

#[derive(Debug)]
pub struct DirEntry {
    path: PathBuf,
    file_type: FileType,
}

impl DirEntry {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn into_path(self) -> PathBuf {
        self.path
    }

    pub fn file_type(&self) -> FileType {
        self.file_type
    }
}

#[derive(Debug)]
enum Kind {
    File,
    Dir,
    Unknown,
}

impl From<&Metadata> for Kind {
    fn from(o: &Metadata) -> Self {
        if o.is_dir() {
            Kind::Dir
        } else if o.is_file() {
            Kind::File
        } else {
            Kind::Unknown
        }
    }
}

#[derive(Debug)]
pub struct WalkDir {
    visited: BTreeSet<PathBuf>,
    unvisited: BTreeMap<PathBuf, Kind>,
}

impl Default for WalkDir {
    fn default() -> Self {
        Self {
            visited: BTreeSet::new(),
            unvisited: BTreeMap::new(),
        }
    }
}

impl WalkDir {
    pub fn new<P>(path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        let mut new = Self::default();
        new.insert(path);
        new
    }

    pub fn insert<P>(&mut self, path: P)
    where
        P: Into<PathBuf>,
    {
        self.unvisited.insert(path.into(), Kind::Unknown);
    }

    async fn step_file(&mut self, path: PathBuf) -> Result<DirEntry, Error> {
        Ok(DirEntry {
            file_type: FileType { is_dir: false },
            path,
        })
    }

    async fn step_dir(&mut self, path: PathBuf) -> Result<DirEntry, Error> {
        let mut readdir = fs::read_dir(&path)
            .await
            .with_context(|| error::ReadDir { path: path.clone() })?;

        let mut err_count = 0;
        loop {
            match readdir.next_entry().await {
                Ok(None) => break,
                Ok(Some(entry)) => {
                    err_count = 0;
                    if let Ok(metadata) = entry.metadata().await {
                        let kind = Kind::from(&metadata);
                        if metadata.is_file() || metadata.is_dir() {
                            self.unvisited.insert(entry.path().into(), kind);
                        }
                    }
                }
                Err(_) => {
                    err_count += 1;
                    if err_count >= 10 {
                        break;
                    }
                }
            }
        }

        Ok(DirEntry {
            path,
            file_type: FileType { is_dir: true },
        })
    }

    async fn step(&mut self) -> Option<Result<DirEntry, Error>> {
        loop {
            let next_path;
            let next_kind;

            loop {
                let (path, kind) = self.unvisited.pop_first()?;
                let res = fs::canonicalize(&path)
                    .await
                    .context(error::Canonicalize { path });

                let canon: PathBuf = match res {
                    Ok(x) => x.into(),
                    Err(e) => return Some(Err(e)),
                };

                if self.visited.insert(canon.clone()) {
                    next_path = canon;
                    next_kind = kind;
                    break;
                }
            }

            match next_kind {
                Kind::File => return Some(self.step_file(next_path).await),
                Kind::Dir => return Some(self.step_dir(next_path).await),
                _ => (),
            }

            let res = fs::metadata(&next_path).await.with_context(|| {
                error::Metadata {
                    path: next_path.clone(),
                }
            });

            let metadata = match res {
                Ok(m) => m,
                Err(e) => return Some(Err(e)),
            };

            if metadata.is_dir() {
                return Some(self.step_dir(next_path).await);
            } else if metadata.is_file() {
                return Some(self.step_file(next_path).await);
            }
        }
    }

    async fn unfold(mut self) -> Option<(Result<DirEntry, Error>, Self)> {
        Some((self.step().await?, self))
    }

    pub fn walk(self) -> impl Stream<Item = Result<DirEntry, Error>> + Send {
        futures::stream::unfold(self, Self::unfold)
    }
}
