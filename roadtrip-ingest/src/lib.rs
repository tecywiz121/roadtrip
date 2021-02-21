pub mod ingest;

pub mod error {
    use snafu::Snafu;

    use std::path::{Path, PathBuf};

    #[derive(Debug, Snafu)]
    #[snafu(visibility = "pub(crate)")]
    pub enum Error {
        #[snafu(context(false))]
        WalkDir {
            source: roadtrip_walkdir::error::Error,
        },
        Ingest {
            source: crate::ingest::Error,
            path: PathBuf,
        },
        Unsupported {
            path: PathBuf,
        },
    }

    impl Error {
        pub fn path(&self) -> &Path {
            use Error::*;

            match self {
                WalkDir { source, .. } => source.path(),
                Ingest { path, .. } => path,
                Unsupported { path, .. } => path,
            }
        }
    }
}

use crate::ingest::{Error as IngestError, Ingest, IngestErase};

use futures::{Stream, StreamExt};

use roadtrip_core::media::Media;

use roadtrip_walkdir::error::Error as WalkError;
use roadtrip_walkdir::{DirEntry, WalkDir};

use self::error::Error;

use snafu::IntoError;

use std::path::PathBuf;
use std::sync::Arc;

type Ingesters = Vec<Box<dyn Ingest<Error = IngestError>>>;

#[derive(Debug)]
pub struct Scanner {
    walkdir: WalkDir,
    ingesters: Ingesters,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    fn new() -> Self {
        Self {
            walkdir: WalkDir::default(),
            ingesters: Vec::new(),
        }
    }

    pub fn add_ingester<I>(&mut self, ingester: I)
    where
        I: 'static + Ingest,
    {
        self.ingesters.push(IngestErase::boxed(ingester));
    }

    pub fn insert_path<P>(&mut self, path: P)
    where
        P: Into<PathBuf>,
    {
        self.walkdir.insert(path);
    }

    async fn step_file(
        ingesters: Arc<Ingesters>,
        path: PathBuf,
    ) -> Result<Media, Error> {
        for ingester in ingesters.iter() {
            match ingester.ingest(path.clone()).await {
                Ok(m) => return Ok(m),
                Err(e) if e.is_supported() => {
                    return Err(error::Ingest { path }.into_error(e))
                }
                Err(_) => (),
            }
        }

        error::Unsupported { path }.fail()
    }

    async fn scan_one(
        ingesters: Arc<Ingesters>,
        result: Result<DirEntry, WalkError>,
    ) -> Option<Result<Media, Error>> {
        match result {
            Ok(e) if e.file_type().is_dir() => None,
            Ok(e) => Some(Self::step_file(ingesters, e.into_path()).await),
            Err(e) => Some(Err(Error::from(e))),
        }
    }

    pub fn scan(self) -> impl Stream<Item = Result<Media, Error>> + Send {
        let walkdir = self.walkdir;

        // TODO: Figure out why this needs to be an Arc, and get rid of it.
        let ingesters = Arc::new(self.ingesters);

        walkdir.walk().filter_map(move |result| {
            let mine = ingesters.clone();
            Self::scan_one(mine, result)
        })
    }
}
