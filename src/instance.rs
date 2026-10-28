//! Single-instance guard.
//!
//! Two Whisper processes on the same data directory (an installed app and a
//! `dist/` build, or `open -n`) each register the global shortcut, record,
//! transcribe and paste, so every dictation lands twice. The first process
//! holds an exclusive `flock` on `instance.lock` in the data directory for its
//! lifetime; later ones exit. The kernel drops the lock when the process
//! dies, so a crash never leaves a stale lock behind.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

const LOCK_FILE_NAME: &str = "instance.lock";

/// Held for the process lifetime; dropping it releases the lock.
pub struct InstanceLock {
    _file: File,
}

pub enum Acquired {
    Locked(InstanceLock),
    /// Another process holds the lock; its pid when the file records one.
    HeldBy(Option<i32>),
}

/// Takes the lock in `dir`, recording this process's pid in the file.
pub fn acquire(dir: &Path) -> io::Result<Acquired> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(LOCK_FILE_NAME))?;
    // SAFETY: `file` owns a valid descriptor for the duration of the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(err);
        }
        let mut contents = String::new();
        let _ = file.read_to_string(&mut contents);
        return Ok(Acquired::HeldBy(contents.trim().parse().ok()));
    }
    file.set_len(0)?;
    file.rewind()?;
    write!(file, "{}", std::process::id())?;
    Ok(Acquired::Locked(InstanceLock { _file: file }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_holder_is_refused_until_the_first_releases() {
        let dir = tempfile::tempdir().unwrap();
        let Acquired::Locked(first) = acquire(dir.path()).unwrap() else {
            panic!("first lock refused");
        };

        match acquire(dir.path()).unwrap() {
            Acquired::HeldBy(pid) => assert_eq!(pid, Some(std::process::id() as i32)),
            Acquired::Locked(_) => panic!("second lock granted"),
        }

        drop(first);
        assert!(matches!(acquire(dir.path()).unwrap(), Acquired::Locked(_)));
    }
}
