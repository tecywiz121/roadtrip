mod exiftool;

use futures::TryFutureExt;

use roadtrip_core::geometry::Geometry;
use roadtrip_core::media::Media;

pub use self::exiftool::Exiftool;

use sha3::{Digest, Sha3_256};

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use tokio::fs::File;
use tokio::io::AsyncReadExt;

async fn create_media(
    path: PathBuf,
    geometry: Geometry,
) -> Result<Media, std::io::Error> {
    let mut file = File::open(&path).await?;
    let mut hasher = Sha3_256::new();

    // TODO: Use st_blksize to get the buffer size
    let mut buf = [0u8; 10240];

    loop {
        let n_read = file.read(&mut buf).await?;
        if n_read == 0 {
            break;
        }

        let read = &buf[0..n_read];
        hasher.update(read);
    }

    let hash = hasher.finalize();
    let array: [u8; 32] = hash.into();

    let media = Media::builder()
        .path(path)
        .geometry(geometry)
        .hash(array.into())
        .build();

    Ok(media)
}

#[derive(Debug)]
pub(crate) struct IngestErase<T>(T);

impl<T> IngestErase<T>
where
    T: 'static + Ingest,
{
    pub fn boxed(t: T) -> Box<dyn Ingest<Error = Error>> {
        Box::new(IngestErase(t))
    }
}

impl<T> Ingest for IngestErase<T>
where
    T: Ingest,
{
    type Error = Error;

    fn ingest<'a>(
        &'a self,
        path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<Media, Self::Error>> + 'a + Send>>
    {
        Box::pin(self.0.ingest(path).map_err(Into::into))
    }
}

pub trait Ingest: std::fmt::Debug + Send + Sync {
    type Error: Into<Error>;

    fn ingest<'a>(
        &'a self,
        path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<Media, Self::Error>> + 'a + Send>>;
}

#[derive(Debug)]
pub struct Error {
    source: Box<dyn std::error::Error + Send + 'static>,
    is_supported: bool,
}

impl Error {
    pub fn new<S>(source: S, supported: bool) -> Self
    where
        S: 'static + std::error::Error + Send,
    {
        Self {
            source: Box::new(source),
            is_supported: supported,
        }
    }

    pub fn is_supported(&self) -> bool {
        self.is_supported
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}
