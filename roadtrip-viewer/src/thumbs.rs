use crate::error::{self, Error, GstError};

use futures::{Stream, StreamExt};

use glib::object::ObjectType;
use glib::{ObjectExt, Value};

use gstreamer::format::GenericFormattedValue;
use gstreamer::{
    self as gst, ClockTime, ElementExt, ElementExtManual, ElementFactory,
    GstBinExtManual, PadExt,
};

use roadtrip_cache::error::{Error as CacheError, InsertError};
use roadtrip_cache::{Cache, Entry, OccupiedEntry, VacantEntry};

use roadtrip_core::media::{Media, Thumbnails};

use snafu::{IntoError, OptionExt};

use std::fs::File as StdFile;
use std::path::PathBuf;
use std::sync::Once;

use tokio::io::AsyncWriteExt;

const CACHE_SIZE: u64 = 10 * 1024 * 1024;

#[derive(Debug)]
pub struct Thumbs {
    cache: Cache,
}

impl Thumbs {
    const INIT: Once = Once::new();

    pub async fn new(root: PathBuf) -> Result<Self, Error> {
        Self::INIT.call_once(|| {
            // TODO: Probably shouldn't call this on behalf of the application.
            gstreamer::init().unwrap();
        });

        let cache = match Cache::new(root, CACHE_SIZE).await {
            Ok(c) => c,
            Err(CacheError::AlreadyLocked) => {
                return Err(Error::AlreadyRunning)
            }
            Err(e) => return Err(error::Cache {}.into_error(e)),
        };

        Ok(Self { cache })
    }

    pub async fn thumbnails(&self, media: &Media) -> Result<Thumbnails, Error> {
        let key = media.hash().to_hex();

        match self.cache.entry(&key).await? {
            Entry::Vacant(v) => self.vacant(media, v).await,
            Entry::Occupied(o) => self.occupied(media, o).await,
        }
    }

    fn pipeline(uri: &str) -> Result<gst::Element, GstError> {
        let afakesink = ElementFactory::make("fakesink", None)?;
        let vfakesink = ElementFactory::make("fakesink", None)?;

        // Crop the video into a square
        let crop = ElementFactory::make("aspectratiocrop", None)?;
        crop.set_property(
            "aspect-ratio",
            &Value::from(&gst::Fraction::new(1, 1)),
        )?;

        // Resize the video to a uniform size.
        let scale = ElementFactory::make("videoscale", None)?;

        // TODO: Remove this hack to set the scale method.
        let method_type = scale.get_property("method")?.type_();
        let method_enum = glib::EnumClass::new(method_type).unwrap();
        let method = method_enum.get_value_by_nick("lanczos").unwrap();
        scale.set_property("method", &method.to_value())?;

        let bin = gst::Bin::new(None);
        bin.add_many(&[&crop, &scale, &vfakesink])?;

        gst::Element::link_many(&[&crop, &scale, &vfakesink])?;

        let pad = crop.get_static_pad("sink").unwrap();
        let ghost = gst::GhostPad::with_target(Some("sink"), &pad)?;

        ghost.set_active(true)?;

        bin.add_pad(&ghost)?;

        let pipeline = gst::parse_launch("playbin")?;

        pipeline.set_property("uri", &Value::from(uri))?;
        pipeline.set_property("audio-sink", &Value::from(&afakesink))?;
        pipeline.set_property("video-sink", &Value::from(&bin))?;

        Ok(pipeline)
    }

    fn capture(pipeline: &gst::Element) -> Result<Vec<u8>, GstError> {
        let caps = gst::Caps::new_simple(
            "image/jpeg",
            &[("width", &200), ("height", &200)],
        );
        let sample = pipeline
            .emit("convert-sample", &[&caps])?
            .context(error::Missing)?;

        let sample = sample.get::<gst::Sample>()?.context(error::Missing)?;

        let buffer = sample.get_buffer().context(error::Missing)?;

        let mut bytes = vec![0u8; buffer.get_size()];
        buffer.copy_to_slice(0, &mut bytes).unwrap();

        Ok(bytes)
    }

    async fn save(
        idx: usize,
        data: Vec<u8>,
        entry: &VacantEntry<'_>,
    ) -> Result<StdFile, InsertError> {
        let name = format!("{:0>2}.jpg", idx);
        let file = entry
            .insert_with(&name, move |mut f| async move {
                f.write_all(&data).await?;
                Ok(())
            })
            .await?
            .into_std()
            .await;
        Ok(file)
    }

    fn when(pipeline: &gst::Element) -> Vec<ClockTime> {
        use self::GenericFormattedValue::Time;

        let mut points = Vec::new();
        let mut query = gst::query::Seeking::new(gst::Format::Time);

        if !pipeline.query(&mut query) {
            return points;
        }

        let start;
        let end;

        match query.get_result() {
            (true, Time(s), Time(e)) => {
                start = s;
                end = e;
            }
            _ => return points,
        }

        let duration = end - start;

        match duration {
            ClockTime(None) | ClockTime(Some(0)) => return points,
            d if d < ClockTime::from_seconds(2) => {
                points.push(d / 2);
            }
            d => {
                let first = ClockTime::from_seconds(1);
                let mid = d - (2 * first);
                let len = mid / 9;
                for c in 0..10 {
                    points.push(first + (c * len));
                }
            }
        }

        points
    }

    async fn vacant<'a>(
        &'a self,
        media: &'a Media,
        entry: VacantEntry<'a>,
    ) -> Result<Thumbnails, Error> {
        let path = media.path().to_str().context(error::Utf8)?;

        let uri = format!("file://{}", path);
        let files = Self::thumbnail(&uri, &entry).await?;

        let thumbnails =
            Thumbnails::new(media.hash().clone(), files.into_iter());
        Ok(thumbnails)
    }

    async fn occupied<'a>(
        &'a self,
        media: &'a Media,
        entry: OccupiedEntry<'a>,
    ) -> Result<Thumbnails, Error> {
        // TODO: Sort the files
        let files = entry.into_files();
        let mut std_files = Vec::new();
        for file in files {
            std_files.push(file.into_file().into_std().await);
        }
        let thumbnails =
            Thumbnails::new(media.hash().clone(), std_files.into_iter());
        Ok(thumbnails)
    }

    async fn until_state<S>(
        stream: &mut S,
        state: gst::State,
    ) -> Result<(), GstError>
    where
        S: Stream<Item = gst::Message> + Unpin + Send,
    {
        use gst::MessageView::*;

        while let Some(msg) = stream.next().await {
            match msg.view() {
                StateChanged(st) => {
                    let cur = st.get_current();
                    let pen = st.get_pending();
                    if cur == state && pen == gst::State::VoidPending {
                        return Ok(());
                    }
                }
                AsyncStart(_) | AsyncDone(_) => {
                    // TODO: Make a more specific error type
                    return Err(GstError::Missing);
                }
                _ => (),
            }
        }

        // TODO: Make a more specific error type
        Err(GstError::Missing)
    }

    async fn until_async_done<S>(stream: &mut S) -> Result<(), GstError>
    where
        S: Stream<Item = gst::Message> + Unpin + Send,
    {
        use gst::MessageView::*;

        while let Some(msg) = stream.next().await {
            match msg.view() {
                AsyncDone(_) => return Ok(()),
                StateChanged(_) | AsyncStart(_) => {
                    // TODO: Make a more specific error type
                    return Err(GstError::Missing);
                }
                _ => (),
            }
        }

        // TODO: Make a more specific error type
        Err(GstError::Missing)
    }

    fn filter_stream(
        pipeline: gst::Element,
    ) -> impl Stream<Item = gst::Message> + Unpin + Send {
        let bus = pipeline.get_bus().unwrap();
        tokio::stream::StreamExt::filter(
            bus.stream_filtered(&[
                gst::MessageType::AsyncStart,
                gst::MessageType::AsyncDone,
                gst::MessageType::StateChanged,
            ]),
            move |msg| match msg.get_src() {
                Some(src) => src.as_object_ref() == pipeline.as_object_ref(),
                None => false,
            },
        )
    }

    async fn thumbnail(
        uri: &str,
        entry: &VacantEntry<'_>,
    ) -> Result<Vec<StdFile>, Error> {
        // TODO: Handle exit events

        let pipeline = Self::pipeline(uri)?;
        let mut stream = Self::filter_stream(pipeline.clone());

        pipeline
            .set_state(gst::State::Paused)
            .map_err(GstError::from)?;

        Self::until_state(&mut stream, gst::State::Paused).await?;

        let points = Self::when(&pipeline);
        let mut files = Vec::with_capacity(std::cmp::max(points.len(), 1));

        if points.is_empty() {
            let bytes = Self::capture(&pipeline)?;
            let file = Self::save(0, bytes, entry).await?;
            files.push(file);
        } else {
            for (idx, point) in points.into_iter().enumerate() {
                pipeline
                    .seek_simple(
                        gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                        point,
                    )
                    .map_err(GstError::from)?;

                Self::until_async_done(&mut stream).await?;
                Self::until_state(&mut stream, gst::State::Paused).await?;

                let bytes = Self::capture(&pipeline)?;
                let file = Self::save(idx, bytes, entry).await?;

                files.push(file);
            }
        }

        pipeline
            .set_state(gst::State::Null)
            .map_err(GstError::from)?;

        Ok(files)
    }
}
