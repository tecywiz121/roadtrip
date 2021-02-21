use crate::geometry::Geometry;
use crate::Hash;

use std::fs::File;
use std::path::{Path, PathBuf};

use typed_builder::TypedBuilder;

#[derive(Debug, TypedBuilder, Clone)]
pub struct Media {
    path: PathBuf,
    geometry: Geometry,
    hash: Hash,
}

impl Media {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    pub fn hash(&self) -> &Hash {
        &self.hash
    }
}

#[derive(Debug)]
pub struct Thumbnails {
    media_hash: Hash,
    files: Vec<File>,
}

impl Thumbnails {
    pub fn new<I>(media_hash: Hash, files: I) -> Self
    where
        I: Iterator<Item = File>,
    {
        Self {
            media_hash,
            files: files.collect(),
        }
    }

    pub fn media_hash(&self) -> &Hash {
        &self.media_hash
    }

    pub fn into_files(self) -> impl Iterator<Item = File> {
        self.files.into_iter()
    }
}
