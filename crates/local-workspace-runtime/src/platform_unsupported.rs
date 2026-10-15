//! Fail-closed placeholder for platforms whose handle-relative, no-follow,
//! reparse-point-safe primitives have not been proven on a real machine.
//!
//! Windows needs its own handle-relative implementation (directory handles
//! opened without FILE_SHARE_DELETE, FILE_FLAG_OPEN_REPARSE_POINT, reparse
//! attribute and file-ID checks, share-deny-write snapshot). Until that is
//! implemented and verified on actual Windows, every operation reports
//! `unavailable` instead of falling back to string-path access.

use std::fs::File;
use std::path::Path;

use crate::error::{RuntimeErrorCode, RuntimeErrorReason, RuntimeResult, err};

// Mirrors the Unix shape so the broker compiles unchanged; never constructed.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Directory,
    Regular,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub kind: Kind,
    pub nlink: u64,
    pub size: u64,
    pub mtime: (i64, i64),
    pub ctime: (i64, i64),
    pub birth: Option<(i64, i64)>,
}

impl Stat {
    pub fn identity(&self) -> (u64, u64) {
        (self.dev, self.ino)
    }
}

fn unsupported<T>() -> RuntimeResult<T> {
    err(
        RuntimeErrorCode::Unavailable,
        RuntimeErrorReason::UnsupportedPlatform,
    )
}

#[derive(Debug)]
pub struct Dir {
    pub stat: Stat,
}

impl Dir {
    pub fn open_absolute(_path: &Path) -> RuntimeResult<Self> {
        unsupported()
    }
    pub fn open_child_dir(&self, _name: &str) -> RuntimeResult<Self> {
        unsupported()
    }
    pub fn open_child_file(&self, _name: &str) -> RuntimeResult<(File, Stat)> {
        unsupported()
    }
    pub fn create_child_file(&self, _name: &str) -> RuntimeResult<File> {
        unsupported()
    }
    pub fn make_private_child_dir(&self, _name: &str) -> RuntimeResult<()> {
        unsupported()
    }
    pub fn stat_child(&self, _name: &str) -> RuntimeResult<Stat> {
        unsupported()
    }
    pub fn unlink_child_file(&self, _name: &str) -> RuntimeResult<()> {
        unsupported()
    }
    pub fn entry_names(&self, _max: usize) -> RuntimeResult<Vec<Vec<u8>>> {
        unsupported()
    }
}

pub fn fstat_file(_file: &File) -> RuntimeResult<Stat> {
    unsupported()
}

pub fn read_stable(_file: &File, _first: &Stat) -> RuntimeResult<Vec<u8>> {
    unsupported()
}

pub fn write_all_durable(_file: &mut File, _bytes: &[u8]) -> RuntimeResult<()> {
    unsupported()
}

#[derive(Debug)]
pub struct InstanceLock;

impl InstanceLock {
    pub fn acquire(_path: &Path) -> RuntimeResult<Self> {
        unsupported()
    }
}

pub fn create_private_dir_all(_path: &Path) -> RuntimeResult<()> {
    unsupported()
}

pub const SUPPORTED: bool = false;
