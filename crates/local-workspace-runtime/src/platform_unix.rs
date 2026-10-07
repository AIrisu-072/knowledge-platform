//! Unix handle-relative, no-follow filesystem primitives.
//!
//! Every operation below a binding root is relative to an already-opened
//! directory descriptor, and every component is opened with `O_NOFOLLOW`.
//! Paths are never re-resolved as strings, so a symlink or a concurrent
//! rename of an ancestor cannot redirect an operation outside the root.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::FileExt;
use std::path::Path;

use crate::error::{RuntimeError, RuntimeErrorCode, RuntimeErrorReason, RuntimeResult, err};

const DIR_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
const READ_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOCTTY;
const CREATE_FLAGS: libc::c_int = libc::O_WRONLY
    | libc::O_CREAT
    | libc::O_EXCL
    | libc::O_NOFOLLOW
    | libc::O_CLOEXEC
    | libc::O_NOCTTY;
const FILE_MODE: libc::c_uint = 0o644;
const PRIVATE_DIR_MODE: libc::mode_t = 0o700;

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
    /// Creation time when the filesystem reports it. Together with dev/ino it
    /// distinguishes a recreated file that reused an inode number.
    pub birth: Option<(i64, i64)>,
}

impl Stat {
    pub fn identity(&self) -> (u64, u64) {
        (self.dev, self.ino)
    }
}

#[allow(clippy::unnecessary_cast)]
fn kind_of(format: u32) -> Kind {
    match format {
        f if f == libc::S_IFDIR as u32 => Kind::Directory,
        f if f == libc::S_IFREG as u32 => Kind::Regular,
        f if f == libc::S_IFLNK as u32 => Kind::Symlink,
        _ => Kind::Other,
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod stat_impl {
    use super::{Kind, Stat, kind_of};

    pub const MASK: libc::c_uint = libc::STATX_BASIC_STATS | libc::STATX_BTIME;

    pub type Raw = libc::statx;

    pub fn convert(st: &Raw) -> Stat {
        #[allow(clippy::unnecessary_cast)]
        let kind: Kind = kind_of(u32::from(st.stx_mode) & libc::S_IFMT as u32);
        let time = |t: libc::statx_timestamp| (t.tv_sec, i64::from(t.tv_nsec));
        Stat {
            dev: (u64::from(st.stx_dev_major) << 32) | u64::from(st.stx_dev_minor),
            ino: st.stx_ino,
            kind,
            nlink: u64::from(st.stx_nlink),
            size: st.stx_size,
            mtime: time(st.stx_mtime),
            ctime: time(st.stx_ctime),
            birth: (st.stx_mask & libc::STATX_BTIME != 0).then(|| time(st.stx_btime)),
        }
    }

    /// statx relative to `dirfd`; an empty name with AT_EMPTY_PATH stats the
    /// descriptor itself.
    pub fn stat_at(
        dirfd: libc::c_int,
        name: &std::ffi::CStr,
        flags: libc::c_int,
    ) -> Result<Stat, i32> {
        let mut st = std::mem::MaybeUninit::<Raw>::uninit();
        // SAFETY: valid descriptor, NUL-terminated name and out-parameter.
        let rc = unsafe { libc::statx(dirfd, name.as_ptr(), flags, MASK, st.as_mut_ptr()) };
        if rc != 0 {
            return Err(super::last_errno());
        }
        // SAFETY: statx succeeded and initialized the requested fields.
        let st = unsafe { st.assume_init() };
        Ok(convert(&st))
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
mod stat_impl {
    use super::{Stat, kind_of};

    #[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
    fn convert(st: &libc::stat) -> Stat {
        #[cfg(target_vendor = "apple")]
        let birth = Some((st.st_birthtime as i64, st.st_birthtime_nsec as i64));
        #[cfg(not(target_vendor = "apple"))]
        let birth = None;
        Stat {
            dev: st.st_dev as u64,
            ino: st.st_ino as u64,
            kind: kind_of(u32::from(st.st_mode & libc::S_IFMT)),
            nlink: st.st_nlink as u64,
            size: u64::try_from(st.st_size).unwrap_or(u64::MAX),
            mtime: (st.st_mtime as i64, st.st_mtime_nsec as i64),
            ctime: (st.st_ctime as i64, st.st_ctime_nsec as i64),
            birth,
        }
    }

    pub fn stat_at(
        dirfd: libc::c_int,
        name: &std::ffi::CStr,
        flags: libc::c_int,
    ) -> Result<Stat, i32> {
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: valid descriptor, NUL-terminated name and out-parameter.
        let rc = if name.is_empty() {
            unsafe { libc::fstat(dirfd, st.as_mut_ptr()) }
        } else {
            unsafe { libc::fstatat(dirfd, name.as_ptr(), st.as_mut_ptr(), flags) }
        };
        if rc != 0 {
            return Err(super::last_errno());
        }
        // SAFETY: the call succeeded and fully initialized `st`.
        let st = unsafe { st.assume_init() };
        Ok(convert(&st))
    }
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Map an errno to a safe code; raw OS text never leaves this module.
pub fn map_errno(errno: i32) -> RuntimeError {
    use RuntimeErrorCode as C;
    use RuntimeErrorReason as R;
    match errno {
        libc::ENOENT | libc::ENOTDIR => RuntimeError::new(C::NotFound),
        // Linux reports ELOOP for an O_NOFOLLOW symlink; some BSDs use EMLINK.
        libc::ELOOP | libc::EMLINK => RuntimeError::with(C::Denied, R::SymbolicLink),
        libc::EACCES | libc::EPERM | libc::EROFS => RuntimeError::with(C::Denied, R::Io),
        libc::EEXIST => RuntimeError::with(C::Conflict, R::AlreadyExists),
        libc::ENAMETOOLONG => RuntimeError::with(C::InvalidLocator, R::InvalidName),
        libc::ENOSPC | libc::EDQUOT | libc::EFBIG => RuntimeError::with(C::Limit, R::TooLarge),
        _ => RuntimeError::with(C::Unavailable, R::Io),
    }
}

fn io_error(error: &std::io::Error) -> RuntimeError {
    map_errno(error.raw_os_error().unwrap_or(0))
}

fn c_name(name: &str) -> RuntimeResult<CString> {
    CString::new(name).map_err(|_| {
        RuntimeError::with(
            RuntimeErrorCode::InvalidLocator,
            RuntimeErrorReason::InvalidName,
        )
    })
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
const EMPTY_PATH_FLAGS: libc::c_int = libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
const EMPTY_PATH_FLAGS: libc::c_int = 0;

fn fstat_fd(fd: RawFd) -> RuntimeResult<Stat> {
    stat_impl::stat_at(fd, c"", EMPTY_PATH_FLAGS).map_err(map_errno)
}

pub fn fstat_file(file: &File) -> RuntimeResult<Stat> {
    fstat_fd(file.as_raw_fd())
}

/// A directory pinned by descriptor together with its identity.
#[derive(Debug)]
pub struct Dir {
    fd: OwnedFd,
    pub stat: Stat,
}

impl Dir {
    fn from_fd(fd: libc::c_int) -> RuntimeResult<Self> {
        if fd < 0 {
            return Err(map_errno(last_errno()));
        }
        // SAFETY: `fd` was just returned by open/openat and is owned here.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let stat = fstat_fd(fd.as_raw_fd())?;
        if stat.kind != Kind::Directory {
            return Err(RuntimeError::new(RuntimeErrorCode::NotFound));
        }
        Ok(Self { fd, stat })
    }

    /// Open an absolute directory the broker itself stored. The final
    /// component must not be a symlink; callers verify identity.
    pub fn open_absolute(path: &Path) -> RuntimeResult<Self> {
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            RuntimeError::with(RuntimeErrorCode::Unavailable, RuntimeErrorReason::Io)
        })?;
        // SAFETY: `path` is a valid NUL-terminated string for the call.
        let fd = unsafe { libc::open(path.as_ptr(), DIR_FLAGS) };
        Self::from_fd(fd)
    }

    pub fn open_child_dir(&self, name: &str) -> RuntimeResult<Self> {
        let c = c_name(name)?;
        // SAFETY: valid directory descriptor and NUL-terminated relative name.
        let fd = unsafe { libc::openat(self.fd.as_raw_fd(), c.as_ptr(), DIR_FLAGS) };
        if fd < 0 {
            let errno = last_errno();
            // O_DIRECTORY|O_NOFOLLOW on a symlink reports ENOTDIR or ELOOP;
            // classify it explicitly so a link is denied, never followed.
            if matches!(errno, libc::ENOTDIR | libc::ELOOP)
                && self.stat_child(name).is_ok_and(|s| s.kind == Kind::Symlink)
            {
                return err(RuntimeErrorCode::Denied, RuntimeErrorReason::SymbolicLink);
            }
            return Err(map_errno(errno));
        }
        Self::from_fd(fd)
    }

    pub fn open_child_file(&self, name: &str) -> RuntimeResult<(File, Stat)> {
        let name = c_name(name)?;
        // SAFETY: valid directory descriptor and NUL-terminated relative name.
        let fd = unsafe { libc::openat(self.fd.as_raw_fd(), name.as_ptr(), READ_FLAGS) };
        if fd < 0 {
            return Err(map_errno(last_errno()));
        }
        // SAFETY: `fd` was just returned by openat and is owned here.
        let file = unsafe { File::from_raw_fd(fd) };
        let stat = fstat_file(&file)?;
        match stat.kind {
            Kind::Regular => Ok((file, stat)),
            Kind::Directory => Err(RuntimeError::new(RuntimeErrorCode::NotFound)),
            Kind::Symlink => err(RuntimeErrorCode::Denied, RuntimeErrorReason::SymbolicLink),
            Kind::Other => err(RuntimeErrorCode::Denied, RuntimeErrorReason::SpecialFile),
        }
    }

    /// Exclusive, no-follow creation of a new regular file.
    pub fn create_child_file(&self, name: &str) -> RuntimeResult<File> {
        let name = c_name(name)?;
        // SAFETY: valid directory descriptor, NUL-terminated relative name and
        // the variadic mode argument required by O_CREAT.
        let fd =
            unsafe { libc::openat(self.fd.as_raw_fd(), name.as_ptr(), CREATE_FLAGS, FILE_MODE) };
        if fd < 0 {
            return Err(map_errno(last_errno()));
        }
        // SAFETY: `fd` was just returned by openat and is owned here.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// Create a private directory; an existing entry is reported as conflict.
    pub fn make_private_child_dir(&self, name: &str) -> RuntimeResult<()> {
        let name = c_name(name)?;
        // SAFETY: valid directory descriptor and NUL-terminated relative name.
        let rc = unsafe { libc::mkdirat(self.fd.as_raw_fd(), name.as_ptr(), PRIVATE_DIR_MODE) };
        if rc != 0 {
            return Err(map_errno(last_errno()));
        }
        Ok(())
    }

    pub fn stat_child(&self, name: &str) -> RuntimeResult<Stat> {
        let name = c_name(name)?;
        stat_impl::stat_at(self.fd.as_raw_fd(), &name, libc::AT_SYMLINK_NOFOLLOW).map_err(map_errno)
    }

    pub fn unlink_child_file(&self, name: &str) -> RuntimeResult<()> {
        let name = c_name(name)?;
        // SAFETY: valid descriptor and relative name; never removes directories.
        let rc = unsafe { libc::unlinkat(self.fd.as_raw_fd(), name.as_ptr(), 0) };
        if rc != 0 {
            return Err(map_errno(last_errno()));
        }
        Ok(())
    }

    /// Raw names of all entries except `.`/`..`, bounded by `max`.
    pub fn entry_names(&self, max: usize) -> RuntimeResult<Vec<Vec<u8>>> {
        // SAFETY: duplicating a live descriptor; the duplicate is handed to
        // fdopendir which takes ownership and closedir releases it.
        let dup = unsafe { libc::fcntl(self.fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if dup < 0 {
            return Err(map_errno(last_errno()));
        }
        // SAFETY: `dup` is a fresh directory descriptor owned by the stream.
        let stream = unsafe { libc::fdopendir(dup) };
        if stream.is_null() {
            let errno = last_errno();
            // SAFETY: fdopendir failed so `dup` is still ours to close.
            unsafe { libc::close(dup) };
            return Err(map_errno(errno));
        }
        // The duplicate shares the offset with earlier streams; start over.
        // SAFETY: `stream` is a valid, open directory stream.
        unsafe { libc::rewinddir(stream) };
        let mut names = Vec::new();
        let mut result = Ok(());
        loop {
            // SAFETY: `stream` is valid; readdir returns null at the end.
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            // SAFETY: readdir returned a valid entry with a NUL-terminated name
            // that stays valid until the next readdir/closedir call.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if names.len() == max {
                result = err(RuntimeErrorCode::Limit, RuntimeErrorReason::TooMany);
                break;
            }
            names.push(name.to_vec());
        }
        // SAFETY: closes the stream and its owned duplicate descriptor.
        unsafe { libc::closedir(stream) };
        result.map(|()| names)
    }
}

#[cfg(target_os = "linux")]
fn read_all_at(file: &File, size: u64) -> RuntimeResult<Vec<u8>> {
    let mut buffer = vec![0u8; usize::try_from(size).unwrap_or(usize::MAX)];
    let mut filled = 0usize;
    while filled < buffer.len() {
        match file.read_at(&mut buffer[filled..], filled as u64) {
            Ok(0) => {
                return err(
                    RuntimeErrorCode::Conflict,
                    RuntimeErrorReason::ConcurrentChange,
                );
            }
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(io_error(&error)),
        }
    }
    // Growth past the observed size is a concurrent change, not a truncation.
    let mut probe = [0u8; 1];
    loop {
        match file.read_at(&mut probe, size) {
            Ok(0) => return Ok(buffer),
            Ok(_) => {
                return err(
                    RuntimeErrorCode::Conflict,
                    RuntimeErrorReason::ConcurrentChange,
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(io_error(&error)),
        }
    }
}

/// Holds a Linux read lease for the duration of a capture. While it is held,
/// any other open for writing (or truncate) of the inode blocks in the kernel
/// until the lease is released, which gives the same mandatory exclusion that
/// a Windows share-deny-write open provides.
/// `F_SETSIG` from the asm-generic fcntl ABI (not exported by `libc` for
/// glibc targets). Only the qualified architectures are listed; others fail
/// closed below.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const F_SETSIG: libc::c_int = 10;

#[cfg(target_os = "linux")]
struct ReadLease<'a> {
    file: &'a File,
}

#[cfg(target_os = "linux")]
impl<'a> ReadLease<'a> {
    fn acquire(file: &'a File) -> RuntimeResult<Self> {
        let fd = file.as_raw_fd();
        // Lease-break notifications use SIGURG, whose default action is to
        // ignore it; the default SIGIO would terminate the process.
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        return err(
            RuntimeErrorCode::Unavailable,
            RuntimeErrorReason::SafeCaptureUnavailable,
        );
        // SAFETY: fcntl on a live descriptor with integer arguments.
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if unsafe { libc::fcntl(fd, F_SETSIG, libc::SIGURG) } != 0 {
            return err(
                RuntimeErrorCode::Unavailable,
                RuntimeErrorReason::SafeCaptureUnavailable,
            );
        }
        // SAFETY: fcntl on a live read-only descriptor with integer arguments.
        if unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_RDLCK) } != 0 {
            return match last_errno() {
                // Another process (or descriptor) has the file open for writing.
                libc::EAGAIN | libc::EBUSY => err(
                    RuntimeErrorCode::Conflict,
                    RuntimeErrorReason::ConcurrentChange,
                ),
                // Not the owner, or the filesystem does not support leases:
                // safe capture cannot be established, so do not read.
                _ => err(
                    RuntimeErrorCode::Unavailable,
                    RuntimeErrorReason::SafeCaptureUnavailable,
                ),
            };
        }
        Ok(Self { file })
    }

    fn held(&self) -> bool {
        // SAFETY: fcntl on a live descriptor with integer arguments.
        unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_GETLEASE) == libc::F_RDLCK }
    }
}

#[cfg(target_os = "linux")]
impl Drop for ReadLease<'_> {
    fn drop(&mut self) {
        // SAFETY: fcntl on a live descriptor with integer arguments.
        unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_SETLEASE, libc::F_UNLCK) };
    }
}

/// Verified stable copy. On Linux a read lease proves no other writer has the
/// inode open and excludes new writers until the copy is finished; two complete
/// reads bracketed by metadata observations (identity, birth, size, link count,
/// nanosecond mtime/ctime) must then agree. Any disagreement, or a lease that
/// was broken meanwhile, rejects the snapshot instead of returning it mixed.
#[cfg(target_os = "linux")]
pub fn read_stable(file: &File, first: &Stat) -> RuntimeResult<Vec<u8>> {
    let changed = || {
        RuntimeError::with(
            RuntimeErrorCode::Conflict,
            RuntimeErrorReason::ConcurrentChange,
        )
    };
    let lease = ReadLease::acquire(file)?;
    // Writes that completed before the lease would show up here.
    if fstat_file(file)? != *first {
        return Err(changed());
    }
    let a = read_all_at(file, first.size)?;
    if fstat_file(file)? != *first {
        return Err(changed());
    }
    let b = read_all_at(file, first.size)?;
    if fstat_file(file)? != *first || a != b || !lease.held() {
        return Err(changed());
    }
    drop(lease);
    Ok(a)
}

/// Other Unix systems offer no mandatory writer exclusion for an arbitrary
/// file, so a safe capture cannot be proven and reads are refused.
#[cfg(not(target_os = "linux"))]
pub fn read_stable(_file: &File, _first: &Stat) -> RuntimeResult<Vec<u8>> {
    err(
        RuntimeErrorCode::Unavailable,
        RuntimeErrorReason::SafeCaptureUnavailable,
    )
}

pub fn write_all_durable(file: &mut File, bytes: &[u8]) -> RuntimeResult<()> {
    use std::io::Write;
    file.write_all(bytes).map_err(|error| io_error(&error))?;
    file.sync_all().map_err(|error| io_error(&error))
}

/// Exclusive advisory lock so only one runtime instance owns a state root.
#[derive(Debug)]
pub struct InstanceLock {
    _fd: OwnedFd,
}

impl InstanceLock {
    pub fn acquire(path: &Path) -> RuntimeResult<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| {
                RuntimeError::with(RuntimeErrorCode::Unavailable, RuntimeErrorReason::Io)
            })?;
        let fd = OwnedFd::from(file);
        // SAFETY: `fd` is a live descriptor owned by this lock.
        let rc = unsafe { libc::flock(fd.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return err(
                RuntimeErrorCode::Unavailable,
                RuntimeErrorReason::InstanceLocked,
            );
        }
        Ok(Self { _fd: fd })
    }
}

pub fn create_private_dir_all(path: &Path) -> RuntimeResult<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|_| RuntimeError::with(RuntimeErrorCode::Unavailable, RuntimeErrorReason::Io))
}

pub const SUPPORTED: bool = true;
