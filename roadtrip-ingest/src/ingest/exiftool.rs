mod error {
    use snafu::Snafu;

    #[derive(Debug, Snafu)]
    #[snafu(visibility = "pub(super)")]
    pub enum Error {
        Spawn {
            source: tokio::io::Error,
        },
        CmdFail {
            status: std::process::ExitStatus,
            err: String,
        },
        Gpx {
            source: gpx::errors::Error,
        },
        Read {
            source: tokio::io::Error,
        },
        NoTimestamp,
    }
}

use roadtrip_core::geometry::{Geometry, Path as CorePath, Point};
use roadtrip_core::media::Media;

pub use self::error::Error;

use snafu::ResultExt;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use super::Ingest;

use tokio::process::Command;

impl From<Error> for super::Error {
    fn from(e: Error) -> Self {
        Self::new(e, true)
    }
}

#[derive(Debug)]
pub struct Exiftool {
    format: PathBuf,
}

impl Exiftool {
    pub const FORMAT: &'static [u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/gpx.fmt"
    ));

    pub fn new(format: PathBuf) -> Self {
        Self { format }
    }

    async fn async_ingest(&self, path: PathBuf) -> Result<Media, Error> {
        let output = Command::new("exiftool")
            .arg("-ee")
            .arg("-p")
            .arg(&self.format)
            .arg("-d")
            .arg("%Y-%m-%dT%H:%M:%SZ")
            .arg(&path)
            .output()
            .await
            .context(error::Spawn)?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_owned();
            return error::CmdFail {
                status: output.status,
                err,
            }
            .fail();
        }

        let gpx = gpx::read(output.stdout.as_slice()).context(error::Gpx)?;
        let meta_time = gpx.metadata.and_then(|m| m.time);

        let mut points: Vec<_> = gpx
            .tracks
            .iter()
            .flat_map(|x| x.segments.iter())
            .flat_map(|x| x.points.iter())
            .map(|x| {
                let point = x.point();
                let time = match x.time {
                    Some(t) => t,
                    None => match meta_time {
                        Some(t) => t,
                        None => return Err(error::NoTimestamp {}.build()),
                    },
                };

                Ok(Point::new(point.lat(), point.lng(), time))
            })
            .collect::<Result<_, _>>()?;

        let geometry = if points.len() == 1 {
            Geometry::from(points.remove(0))
        } else {
            Geometry::from(CorePath::from_iter(points))
        };

        let media = super::create_media(path, geometry)
            .await
            .context(error::Read)?;

        Ok(media)
    }
}

impl Ingest for Exiftool {
    type Error = Error;

    fn ingest<'a>(
        &'a self,
        path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<Media, Error>> + 'a + Send>> {
        Box::pin(self.async_ingest(path))
    }
}
