use snafu::Snafu;

use std::fmt;
use std::path::PathBuf;

use super::Command;

use tokio::sync::mpsc::error::SendError as TokioSendError;

pub struct SendError {
    _p: (),
}

impl std::error::Error for SendError {}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "channel disconnected")
    }
}

impl fmt::Debug for SendError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SendError")
    }
}

impl From<TokioSendError<Command>> for SendError {
    fn from(_: TokioSendError<Command>) -> Self {
        SendError { _p: () }
    }
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum GstError {
    Missing,
    #[snafu(context(false))]
    StateChange {
        source: gstreamer::StateChangeError,
    },
    #[snafu(context(false))]
    GlibBool {
        source: glib::error::BoolError,
    },
    #[snafu(context(false))]
    Glib {
        source: glib::error::Error,
    },
    #[snafu(context(false))]
    GlibGet {
        source: glib::value::GetError,
    },
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum Error {
    Utf8,
    Directories,
    Fs {
        source: std::io::Error,
        path: PathBuf,
    },
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
    Join {
        what: &'static str,
        source: tokio::task::JoinError,
    },
    #[snafu(context(false))]
    Thumbnail {
        source: GstError,
    },
    AlreadyRunning,
}
