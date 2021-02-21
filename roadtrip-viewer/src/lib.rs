pub mod dirs;
pub mod error;
mod exit;
mod thumbs;

use crate::dirs::Dirs;
use crate::error::{Error, SendError};
use crate::exit::Exit;
use crate::thumbs::Thumbs;

use futures::{pin_mut, Stream, StreamExt};

use roadtrip_core::geometry::Filter;
use roadtrip_core::media::{Media, Thumbnails};

use roadtrip_ingest::ingest::Exiftool;
use roadtrip_ingest::Scanner;

use snafu::{IntoError, OptionExt, ResultExt};

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

#[derive(Debug)]
struct State {
    dirs: Dirs,
    thumbs: Thumbs,
    filter: RwLock<Option<Filter>>,
    scans: Mutex<usize>,
    events: Sender<Event>,
    exit: Exit,
}

impl State {
    pub async fn new(events: Sender<Event>) -> Result<Self, Error> {
        let dirs = Dirs::new().context(error::Directories)?;
        let thumbs_dir = dirs.cache_dir().await?.join("thumbnails");

        fs::create_dir_all(&thumbs_dir)
            .await
            .with_context(|| error::Fs {
                path: thumbs_dir.clone(),
            })?;

        let new = Self {
            thumbs: Thumbs::new(thumbs_dir).await?,
            filter: RwLock::new(None),
            scans: Mutex::new(0),
            exit: Exit::new(),
            dirs,
            events,
        };

        Ok(new)
    }

    async fn start_scan(&self) {
        let mut scans = self.scans.lock().await;

        if 0 == *scans {
            self.events.clone().send(Event::MediaScanStarted).await.ok();
        }

        *scans += 1;
    }

    async fn stop_scan(&self) {
        let mut scans = self.scans.lock().await;

        if 1 == *scans {
            self.events
                .clone()
                .send(Event::MediaScanCompleted)
                .await
                .ok();
        }

        *scans -= 1;
    }
}

#[derive(Debug)]
pub enum Event {
    MediaScanStarted,
    MediaScanCompleted,
    MediaScanError(roadtrip_ingest::error::Error),

    FilterMatched(Media),
    FilterChanged,

    Thumbnails(Thumbnails),

    Error(Error),
}

#[derive(Debug)]
enum Command {
    ScanMedia(PathBuf),
    Filter(Option<Filter>),
}

impl Command {
    async fn run(self, state: &Arc<State>) -> Result<(), Error> {
        match self {
            Command::ScanMedia(path) => {
                Self::scan_media(path, state.clone()).await
            }
            Command::Filter(filter) => Self::filter(filter, state).await,
        }
    }

    async fn filter(
        filter: Option<Filter>,
        state: &Arc<State>,
    ) -> Result<(), Error> {
        let mut old = state.filter.write().await;

        if *old == filter {
            return Ok(());
        }

        *old = filter;
        state.events.clone().send(Event::FilterChanged).await.ok();

        Ok(())
    }

    async fn write_exiftool_format(state: &State) -> Result<PathBuf, Error> {
        let path = state.dirs.data_local_dir().await?.join("gpx.fmt");

        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await;

        let mut file = match opened {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(path)
            }
            Err(e) => return Err(error::Fs { path }.into_error(e)),
        };

        file.write_all(Exiftool::FORMAT)
            .await
            .with_context(|| error::Fs { path: path.clone() })?;

        Ok(path)
    }

    fn thumbnail(media: Media, state: Arc<State>) {
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async move {
                let mut events = state.events.clone();

                match state.thumbs.thumbnails(&media).await {
                    Ok(t) => {
                        events.send(Event::Thumbnails(t)).await.ok();
                    }
                    Err(err) => {
                        events
                            .send(Event::Error(err))
                            .await
                            .expect("unable to send error event");
                    }
                }
            });
        });
    }

    async fn scan_media(path: PathBuf, state: Arc<State>) -> Result<(), Error> {
        let mut scanner = Scanner::default();

        let format_path = Self::write_exiftool_format(&state).await?;
        let ingester = Exiftool::new(format_path);

        scanner.add_ingester(ingester);
        scanner.insert_path(path);

        tokio::spawn(async move {
            let mut events = state.events.clone();

            state.start_scan().await;

            let stream = scanner.scan();
            pin_mut!(stream);

            let mut exit = state.exit.from(stream).await;

            while let Some(media_res) = exit.next().await {
                let media = match media_res {
                    Ok(m) => m,
                    Err(e) => {
                        events.send(Event::MediaScanError(e)).await.ok();
                        continue;
                    }
                };

                let opt_filter = state.filter.read().await;
                if let Some(filter) = &*opt_filter {
                    if media.geometry().matches(filter) {
                        Self::thumbnail(media.clone(), state.clone());
                        events.send(Event::FilterMatched(media)).await.ok();
                    }
                }
            }

            state.stop_scan().await;
        });

        Ok(())
    }
}

#[derive(Debug)]
pub struct Viewer {
    handle: Handle,
    events: Receiver<Event>,
    join: JoinHandle<()>,
}

impl Viewer {
    async fn run(recv: Receiver<Command>, state: Arc<State>) {
        let mut events = state.events.clone();
        let mut cmds = state.exit.from(recv).await;

        while let Some(cmd) = cmds.next().await {
            if let Err(e) = cmd.run(&state).await {
                events
                    .send(Event::Error(e))
                    .await
                    .expect("unable to send error event");
            }
        }

        state.exit.exit().await;
    }

    pub async fn spawn() -> Result<Self, Error> {
        let (event_sender, event_receiver) = channel(5);
        let (cmd_sender, cmd_receiver) = channel(5);
        let state = Arc::new(State::new(event_sender).await?);
        let exit = state.exit.clone();

        let join = tokio::spawn(Self::run(cmd_receiver, state));

        Ok(Self {
            handle: Handle {
                sender: cmd_sender,
                exit,
            },
            events: event_receiver,
            join,
        })
    }

    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    pub fn events(self) -> impl Stream<Item = Event> + Unpin {
        // TODO: Maybe await self.join after last event?
        self.events
    }
}

impl Deref for Viewer {
    type Target = Handle;

    fn deref(&self) -> &Handle {
        &self.handle
    }
}

impl DerefMut for Viewer {
    fn deref_mut(&mut self) -> &mut Handle {
        &mut self.handle
    }
}

#[derive(Debug, Clone)]
pub struct Handle {
    sender: Sender<Command>,
    exit: Exit,
}

impl Handle {
    pub async fn exit(&self) {
        self.exit.exit().await;
    }

    pub async fn scan_media<P>(&mut self, path: P) -> Result<(), SendError>
    where
        P: Into<PathBuf>,
    {
        self.sender.send(Command::ScanMedia(path.into())).await?;
        Ok(())
    }

    pub async fn filter<F>(&mut self, filter: F) -> Result<(), SendError>
    where
        F: Into<Option<Filter>>,
    {
        self.sender.send(Command::Filter(filter.into())).await?;
        Ok(())
    }

    pub fn into_sync(self) -> SyncHandle {
        SyncHandle {
            handle: self,
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncHandle {
    handle: Handle,
    runtime: tokio::runtime::Handle,
}

impl SyncHandle {
    pub fn exit(&self) {
        self.runtime.block_on(self.handle.exit())
    }

    pub fn filter<F>(&mut self, filter: F) -> Result<(), SendError>
    where
        F: Into<Option<Filter>>,
    {
        self.runtime.block_on(self.handle.filter(filter))
    }

    pub fn scan_media<P>(&mut self, path: P) -> Result<(), SendError>
    where
        P: Into<PathBuf>,
    {
        self.runtime.block_on(self.handle.scan_media(path))
    }
}
