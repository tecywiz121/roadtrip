use roadtrip_core::geometry::Filter;

use roadtrip_viewer::{Event, Viewer};

use std::fmt::Display;
use std::future::Future;
use std::time::Duration;

use tokio::stream::StreamExt;
use tokio::time::Timeout;

const TM: Duration = Duration::from_secs(10);

const MEDIA_DIR: &'static str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/media");

#[derive(Debug)]
struct Failure(String);

impl<E> From<E> for Failure
where
    E: 'static + std::error::Error,
{
    fn from(e: E) -> Self {
        Failure(e.to_string())
    }
}

trait Ensure {
    type Output;

    fn ensure<S>(self, msg: S) -> Result<Self::Output, Failure>
    where
        S: Display;
}

impl<T, E> Ensure for Result<T, E>
where
    E: Display,
{
    type Output = T;

    fn ensure<S>(self, msg: S) -> Result<T, Failure>
    where
        S: Display,
    {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err(Failure(format!("{} ({})", msg, e))),
        }
    }
}

impl<T> Ensure for Option<T> {
    type Output = T;

    fn ensure<S>(self, msg: S) -> Result<T, Failure>
    where
        S: Display,
    {
        match self {
            Some(t) => Ok(t),
            None => Err(Failure(msg.to_string())),
        }
    }
}

impl Ensure for bool {
    type Output = ();

    fn ensure<S>(self, msg: S) -> Result<(), Failure>
    where
        S: Display,
    {
        if self {
            Ok(())
        } else {
            Err(Failure(msg.to_string()))
        }
    }
}

trait Tm: Sized {
    fn tm(self) -> Timeout<Self>;
}

impl<T> Tm for T
where
    T: Future,
{
    fn tm(self) -> Timeout<Self> {
        tokio::time::timeout(TM, self)
    }
}

#[tokio::test]
async fn scan_media() -> Result<(), Failure> {
    let viewer = Viewer::spawn().tm().await??;
    let mut handle = viewer.handle().clone();
    let mut events = viewer.events();

    let filter = Filter::default();

    handle.filter(filter).tm().await??;

    let e0 = events.next().tm().await?.ensure("missing filter change")?;
    matches!(e0, Event::FilterChanged).ensure("not filter change")?;

    handle.scan_media(MEDIA_DIR).tm().await??;

    let e1 = events.next().tm().await?.ensure("missing scan started")?;
    matches!(e1, Event::MediaScanStarted).ensure("not scan started")?;

    let e2 = events.next().tm().await?.ensure("missing filter matched")?;
    let media = match e2 {
        Event::FilterMatched(f) => f,
        _ => panic!("not filter matched"),
    };

    let e3 = events.next().tm().await?.ensure("missing scan completed")?;
    matches!(e3, Event::MediaScanCompleted).ensure("not scan completed")?;

    let e4 = events.next().tm().await?.ensure("missing thumbnails")?;
    matches!(e4, Event::Thumbnails(_)).ensure("not thumbnails")?;

    let expected: [u8; 32] = [
        208, 99, 183, 103, 68, 222, 159, 245, 183, 210, 136, 232, 193, 245,
        158, 129, 205, 28, 191, 234, 25, 11, 250, 231, 247, 49, 18, 212, 225,
        143, 140, 25,
    ];

    assert_eq!(media.hash().0, expected);

    Ok(())
}
