pub mod datetime;
pub mod geometry;
pub mod media;

use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub fn from_slice(slice: &[u8]) -> Self {
        let mut new = Hash([0; 32]);
        new.copy_from_slice(slice);
        new
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }
}

impl Deref for Hash {
    type Target = [u8; 32];

    fn deref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl DerefMut for Hash {
    fn deref_mut(&mut self) -> &mut [u8; 32] {
        &mut self.0
    }
}

impl From<Hash> for [u8; 32] {
    fn from(other: Hash) -> Self {
        other.0
    }
}

impl From<[u8; 32]> for Hash {
    fn from(other: [u8; 32]) -> Self {
        Hash(other)
    }
}
