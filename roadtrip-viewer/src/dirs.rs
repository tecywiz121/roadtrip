use crate::error::{self, Error};

use directories::ProjectDirs;

use snafu::ResultExt;

use std::path::Path;

use tokio::fs::create_dir_all;

#[derive(Debug)]
pub struct Dirs(ProjectDirs);

impl Dirs {
    pub fn new() -> Option<Self> {
        let inner =
            ProjectDirs::from("rocks.tabby", "Tabby Rocks", "roadtrip")?;

        Some(Dirs(inner))
    }

    pub async fn data_local_dir(&self) -> Result<&Path, Error> {
        let path = self.0.data_local_dir();
        create_dir_all(&path)
            .await
            .with_context(|| error::Fs { path })?;
        Ok(path)
    }

    pub async fn cache_dir(&self) -> Result<&Path, Error> {
        let path = self.0.cache_dir();
        create_dir_all(&path)
            .await
            .with_context(|| error::Fs { path })?;
        Ok(path)
    }
}
