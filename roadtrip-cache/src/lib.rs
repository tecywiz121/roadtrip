pub mod error;
mod lock;

use crate::error::{EntryError, Error, InsertError};
use crate::lock::Lock;

use filetime::{set_file_handle_times, FileTime};

use futures::{pin_mut, StreamExt, TryStreamExt};

use linked_hash_map as lhm;

use roadtrip_walkdir::WalkDir;

use snafu::{ensure, IntoError, ResultExt};

use std::collections::HashMap;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use tokio::fs::{self, File, OpenOptions, ReadDir};
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct NamedFile {
    name: String,
    file: File,
}

impl Deref for NamedFile {
    type Target = File;

    fn deref(&self) -> &File {
        &self.file
    }
}

impl DerefMut for NamedFile {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

impl NamedFile {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn into_file(self) -> File {
        self.file
    }
}

#[derive(Debug)]
pub struct OccupiedEntry<'a> {
    cache: &'a Cache,
    path: PathBuf,
    files: Vec<NamedFile>,
}

impl<'a> OccupiedEntry<'a> {
    pub fn into_files(self) -> impl Iterator<Item = NamedFile> {
        self.files.into_iter()
    }
}

#[derive(Debug)]
pub struct VacantEntry<'a> {
    cache: &'a Cache,
    path: PathBuf,
}

impl<'a> VacantEntry<'a> {
    pub async fn insert_with<F, O>(
        &self,
        name: &str,
        f: F,
    ) -> Result<File, InsertError>
    where
        F: FnOnce(File) -> O,
        O: Future<Output = Result<(), std::io::Error>>,
    {
        ensure!(check_path(name), error::InvalidName);

        match fs::create_dir(&self.path).await {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => {
                return Err(error::Create {
                    path: self.path.clone(),
                }
                .into_error(e))
            }
        }

        let path = self.path.join(name);
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
            .with_context(|| error::Create { path: path.clone() })?;

        // TODO: Shouldn't need to clone this. The closure `f` should be able to
        //       accept an `&mut File`...
        let file2 = file
            .try_clone()
            .await
            .with_context(|| error::Create { path: path.clone() })?;

        f(file2)
            .await
            .with_context(|| error::Write { path: path.clone() })?;

        file.sync_all()
            .await
            .with_context(|| error::Write { path: path.clone() })?;

        let info = file
            .metadata()
            .await
            .with_context(|| error::Metadata { path: path.clone() })?;

        let ro = File::open(&path)
            .await
            .with_context(|| error::Reopen { path: path.clone() })?;

        drop(file);

        self.cache
            .insert(self.path.clone(), info.len())
            .await
            .context(error::Reserve)?;

        Ok(ro)
    }
}

#[derive(Debug)]
pub enum Entry<'a> {
    Occupied(OccupiedEntry<'a>),
    Vacant(VacantEntry<'a>),
}

#[derive(Debug)]
pub struct Cache {
    lock: Lock,
    root: PathBuf,
    items: Mutex<lhm::LinkedHashMap<PathBuf, u64>>,
    capacity: u64,
}

impl Cache {
    pub async fn new<P>(root: P, capacity: u64) -> Result<Self, Error>
    where
        P: Into<PathBuf>,
    {
        let root = root.into();

        let lock_path = root.join(".lock");
        let lock_result = tokio::task::spawn_blocking(|| Lock::new(lock_path))
            .await
            .context(error::LockJoin)?;

        let lock = match lock_result {
            Ok(l) => l,
            Err(lock::Error::AlreadyLocked) => {
                return Err(Error::AlreadyLocked)
            }
            Err(source) => return Err(Error::Lock { source }),
        };

        // TODO: The whole canonicalize nonsense in walkdir is probably gratuitous.
        let canon =
            fs::canonicalize(&root).await.context(error::Canonicalize)?;

        let mut items: HashMap<PathBuf, (FileTime, u64)> = HashMap::new();

        let walkdir = WalkDir::new(&canon).walk();
        pin_mut!(walkdir);

        while let Some(result) = walkdir.next().await {
            let entry = result?;

            if entry.file_type().is_dir() {
                continue;
            }

            let relative = match entry.path().strip_prefix(&canon) {
                Ok(r) if r == Path::new(".lock") => continue,
                Ok(r) => r,
                Err(_) => continue,
            };

            let components: Vec<_> = relative.iter().collect();
            ensure!(
                components.len() == 2,
                error::Structure {
                    path: entry.path().clone()
                }
            );

            let metadata =
                fs::metadata(entry.path()).await.with_context(|| {
                    error::Size {
                        path: entry.path().clone(),
                    }
                })?;

            let ft = FileTime::from_last_modification_time(&metadata);

            let key = root.join(components[0]);

            let mut ft_sz = items.entry(key).or_insert((FileTime::zero(), 0));
            ft_sz.0 = std::cmp::max(ft_sz.0, ft);
            ft_sz.1 += metadata.len();
        }

        let mut sorted: Vec<_> = items.into_iter().collect();
        sorted.sort_by_key(|(_, (tm, _))| *tm);

        let packed = sorted
            .into_iter()
            .map(|(path, (_, sz))| (path, sz))
            .collect();

        Ok(Self {
            items: Mutex::new(packed),
            lock,
            root,
            capacity,
        })
    }

    async fn vacant_entry<'a>(
        &'a self,
        path: PathBuf,
    ) -> Result<VacantEntry<'a>, EntryError> {
        Ok(VacantEntry { cache: self, path })
    }

    async fn spawn_update_mtime(
        file: &File,
        now: FileTime,
    ) -> Result<(), EntryError> {
        let clone = file
            .try_clone()
            .await
            .context(error::FileTime)?
            .into_std()
            .await;

        let result = tokio::task::spawn_blocking(move || {
            // TODO: This can probably be done asynchronously
            set_file_handle_times(&clone, None, Some(now))
                .context(error::FileTime)
        })
        .await
        .context(error::Join)??;

        Ok(result)
    }

    async fn occupied_entry<'a>(
        &'a self,
        path: PathBuf,
        dirs: ReadDir,
    ) -> Result<OccupiedEntry<'a>, EntryError> {
        let now = FileTime::now();

        let files = dirs
            .filter_map(|x| async {
                // TODO: Report these errors.
                let entry = x.ok()?;
                let file_type = entry.file_type().await.ok()?;

                if file_type.is_file() {
                    // Recover the name from the path.
                    let name = match entry.path().strip_prefix(&path) {
                        Ok(n) => n.to_string_lossy().into_owned(),
                        Err(e) => {
                            return Some(Err(EntryError::Prefix { source: e }))
                        }
                    };

                    // Try to open the file.
                    let result =
                        File::open(entry.path()).await.with_context(|| {
                            error::Open {
                                path: entry.path().to_owned(),
                            }
                        });

                    let file = match result {
                        Ok(f) => f,
                        Err(e) => return Some(Err(e)),
                    };

                    // Spawn and wait for a task to update the file's mtime.
                    if let Err(e) = Self::spawn_update_mtime(&file, now).await {
                        return Some(Err(e));
                    }

                    Some(Ok(NamedFile { name, file }))
                } else {
                    None
                }
            })
            .try_collect()
            .await?;

        let unexpected = self.items.lock().await.get_refresh(&path).is_none();

        if unexpected {
            panic!("unexpected directory: {:?}", path);
        }

        Ok(OccupiedEntry {
            cache: self,
            files,
            path,
        })
    }

    pub async fn entry<'a>(
        &'a self,
        key: &'a str,
    ) -> Result<Entry<'a>, EntryError> {
        let path = self.to_path(key)?;

        match fs::read_dir(&path).await {
            Ok(dirs) => {
                self.occupied_entry(path, dirs).await.map(Entry::Occupied)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.vacant_entry(path).await.map(Entry::Vacant)
            }
            Err(e) => Err(error::ReadDir { path }.into_error(e)),
        }
    }

    fn to_path(&self, key: &str) -> Result<PathBuf, EntryError> {
        ensure!(check_path(key), error::InvalidKey);
        let path = self.root.join(key);
        Ok(path)
    }

    pub async fn size(&self) -> u64 {
        let items = self.items.lock().await;
        items.values().sum()
    }

    pub async fn len(&self) -> usize {
        let items = self.items.lock().await;
        items.len()
    }

    pub async fn capacity(&self) -> u64 {
        self.capacity
    }

    async fn insert(
        &self,
        path: PathBuf,
        new_sz: u64,
    ) -> Result<(), std::io::Error> {
        let mut map = self.items.lock().await;
        let size: u64 = map.values().sum();
        let available = if self.capacity >= size {
            self.capacity - size
        } else {
            0
        };

        if available < new_sz {
            let missing = new_sz - available;
            let mut removed = 0;

            let mut entries = map.entries();

            while removed < missing {
                let entry = match entries.next() {
                    Some(i) => i,
                    None => break,
                };

                if entry.key() == &path {
                    continue;
                }

                fs::remove_dir_all(entry.key()).await?;
                removed += entry.remove();
            }
        }

        *map.entry(path).or_insert(0) += new_sz;
        Ok(())
    }
}

fn check_path(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        None | Some('.') => false,
        Some(c) if !c.is_alphanumeric() => false,
        _ => chars.all(|x| '.' == x || x.is_alphanumeric()),
    }
}
