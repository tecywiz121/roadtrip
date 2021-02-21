use fd_lock::{FdLock, FdLockGuard};

use snafu::{ResultExt, Snafu};

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::marker::PhantomPinned;
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[derive(Debug, Snafu)]
pub enum Error {
    AlreadyLocked,
    Create { source: io::Error },
    Other,
}

#[derive(Debug)]
struct Inner {
    _pin: PhantomPinned,
    path: PathBuf,
    lock: FdLock<File>,
    guard: *mut c_void,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        if !self.guard.is_null() {
            unsafe {
                Box::from_raw(self.guard as *mut FdLockGuard<'_, File>);
            }
            self.guard = std::ptr::null_mut();
        }
    }
}

impl Inner {
    pub fn new(path: &Path) -> Result<Pin<Box<Self>>, Error> {
        let path = path.into();
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path)
            .context(Create)?;

        let lock = FdLock::new(file);

        let inner = Self {
            _pin: PhantomPinned,
            path,
            lock,
            guard: std::ptr::null_mut(),
        };

        let mut boxed = Box::pin(inner);

        unsafe {
            let mut_ref: Pin<&mut Self> = Pin::as_mut(&mut boxed);
            let oaeu = Pin::get_unchecked_mut(mut_ref);
            let lock_res = oaeu.lock.try_lock();

            let guard = match lock_res {
                Ok(g) => g,
                Err(e) if matches!(e.kind(), fd_lock::ErrorKind::Locked) => {
                    return Err(Error::AlreadyLocked);
                }
                Err(_) => return Err(Error::Other),
            };

            oaeu.guard = Box::into_raw(Box::new(guard)) as *mut c_void;
        }

        Ok(boxed)
    }
}

#[derive(Debug)]
pub struct Lock {
    inner: Option<Pin<Box<Inner>>>,
    path: PathBuf,
}

impl Lock {
    pub fn new<P>(p: P) -> Result<Self, Error>
    where
        P: Into<PathBuf>,
    {
        let path = p.into();
        Ok(Self {
            inner: Some(Inner::new(&path)?),
            path,
        })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            drop(inner);
            std::fs::remove_file(&self.path).ok();
        }
    }
}
