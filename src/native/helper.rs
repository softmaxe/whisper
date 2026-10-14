//! Shared plumbing for the bundled helper processes: locating binaries,
//! verifying their architecture, and running them with deadlines.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::paths;

use super::timer::lock;

const CPU_TYPE_ARM64: i32 = 0x0100_000c;
const CPU_TYPE_X86_64: i32 = 0x0100_0007;
const MH_MAGIC_64: u32 = 0xfeed_facf;

/// Resolves an executable helper from the bundle (`Contents/Resources/bin`)
/// or the development `resources/bin`, fixing a missing executable bit.
/// Returns `None` when no candidate exists.
pub fn resolve_helper(name: &str) -> Option<PathBuf> {
    let mut candidates = vec![paths::helper(name)];
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("bin")
        .join(name);
    if !candidates.contains(&dev) {
        candidates.push(dev);
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file() && ensure_executable(candidate))
}

fn ensure_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if metadata.permissions().mode() & 0o111 != 0 {
        return true;
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).is_ok()
}

/// Checks a helper is a 64-bit Mach-O for the running architecture.
/// Returns a human-readable mismatch, or `None` when it matches or the check
/// itself fails (verification is best-effort).
pub fn arch_mismatch(path: &Path, label: &str) -> Option<String> {
    let mut header = [0u8; 8];
    let read = std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut header));
    if read.is_err() {
        return None;
    }
    let magic = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    if magic != MH_MAGIC_64 {
        return Some(format!(
            "{label} binary is not a valid 64-bit Mach-O file (magic: 0x{magic:x})"
        ));
    }
    let cpu = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let expected = if cfg!(target_arch = "aarch64") {
        CPU_TYPE_ARM64
    } else {
        CPU_TYPE_X86_64
    };
    if cpu == expected {
        return None;
    }
    let binary_arch = match cpu {
        CPU_TYPE_ARM64 => "arm64".to_string(),
        CPU_TYPE_X86_64 => "x86_64".to_string(),
        other => format!("unknown(0x{other:x})"),
    };
    Some(format!(
        "{label} binary architecture mismatch: binary is {binary_arch} but this Mac requires {}. \
         The app may have been built incorrectly.",
        std::env::consts::ARCH
    ))
}

/// Sends a signal to a process id. Errors are ignored: the process may be gone.
pub fn signal_pid(pid: u32, signal: i32) {
    if pid == 0 {
        return;
    }
    // SAFETY: kill(2) has no memory-safety preconditions.
    unsafe {
        libc::kill(pid as libc::pid_t, signal);
    }
}

/// Outcome of [`run_with_timeout`]. `status` is `None` when the process could
/// not be spawned or never exited before the deadline.
#[derive(Debug, Clone, Default)]
pub struct RunOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl RunOutput {
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
}

/// Output beyond this many bytes is dropped.
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

fn spawn_collector<R: Read + Send + 'static>(
    mut reader: R,
    sink: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut buffer = lock(&sink);
                    if buffer.len() < MAX_OUTPUT_BYTES {
                        buffer.extend_from_slice(&chunk[..n]);
                    }
                }
            }
        }
    })
}

/// Runs `program args` with a deadline and never blocks past it.
///
/// A helper can exit while a descendant keeps its pipes open, so an exit code
/// already reported when the deadline passes is honoured instead of being
/// treated as a timeout.
pub fn run_with_timeout(
    program: &Path,
    args: &[&str],
    stdin: Option<&str>,
    timeout: Duration,
) -> RunOutput {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return RunOutput {
                status: None,
                stdout: String::new(),
                stderr: error.to_string(),
                timed_out: false,
            };
        }
    };
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(input.as_bytes());
        drop(pipe);
    }

    let stdout = Arc::new(Mutex::new(Vec::new()));
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|pipe| spawn_collector(pipe, Arc::clone(&stdout))),
        child
            .stderr
            .take()
            .map(|pipe| spawn_collector(pipe, Arc::clone(&stderr))),
    ]
    .into_iter()
    .flatten()
    .collect();

    let deadline = Instant::now() + timeout;
    let mut exit: Option<Option<i32>> = None;
    let mut timed_out = false;
    loop {
        if exit.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => exit = Some(status.code()),
                Ok(None) => {}
                Err(_) => exit = Some(None),
            }
        }
        if exit.is_some() && readers.iter().all(|reader| reader.is_finished()) {
            break;
        }
        if Instant::now() >= deadline {
            if exit.is_none() {
                timed_out = true;
                let _ = child.kill();
            }
            reap_in_background(child);
            return RunOutput {
                status: exit.flatten(),
                stdout: String::from_utf8_lossy(&lock(&stdout)).into_owned(),
                stderr: String::from_utf8_lossy(&lock(&stderr)).into_owned(),
                timed_out,
            };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    for reader in readers {
        let _ = reader.join();
    }
    let output = RunOutput {
        status: exit.flatten(),
        stdout: String::from_utf8_lossy(&lock(&stdout)).into_owned(),
        stderr: String::from_utf8_lossy(&lock(&stderr)).into_owned(),
        timed_out: false,
    };
    let _ = child.wait();
    output
}

fn reap_in_background(mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Spawns a probe whose verdict is its first output line. Stdin is closed at
/// once; the child is killed after the verdict or at the deadline. Returns
/// `None` when the probe could not run or produced nothing in time.
///
/// A stale helper that predates a probe flag falls into monitor mode and keeps
/// running; resolving on the first line keeps that a fast fallback.
pub fn probe_first_line(program: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    drop(child.stdin.take());
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let mut buffered = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => {
                    let text = String::from_utf8_lossy(&buffered).trim().to_string();
                    let _ = tx.send(Some(text));
                    return;
                }
                Ok(n) => {
                    buffered.extend_from_slice(&chunk[..n]);
                    if let Some(newline) = buffered.iter().position(|b| *b == b'\n') {
                        let line = String::from_utf8_lossy(&buffered[..newline])
                            .trim()
                            .to_string();
                        let _ = tx.send(Some(line));
                        return;
                    }
                }
            }
        }
    });
    let verdict = rx.recv_timeout(timeout).ok().flatten();
    let _ = child.kill();
    reap_in_background(child);
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh() -> PathBuf {
        PathBuf::from("/bin/sh")
    }

    #[test]
    fn run_collects_output_and_status() {
        let out = run_with_timeout(
            &sh(),
            &["-c", "echo hi; echo err >&2; exit 3"],
            None,
            Duration::from_secs(5),
        );
        assert_eq!(out.status, Some(3));
        assert_eq!(out.stdout.trim(), "hi");
        assert_eq!(out.stderr.trim(), "err");
        assert!(!out.timed_out);
    }

    #[test]
    fn run_times_out_a_hung_process() {
        let started = Instant::now();
        let out = run_with_timeout(
            &sh(),
            &["-c", "exec sleep 5"],
            None,
            Duration::from_millis(100),
        );
        assert!(out.timed_out);
        assert_eq!(out.status, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn run_honours_an_exit_code_while_a_descendant_holds_the_pipes() {
        let out = run_with_timeout(
            &sh(),
            &["-c", "sleep 3 & echo done; exit 0"],
            None,
            Duration::from_millis(300),
        );
        assert_eq!(out.status, Some(0));
        assert!(!out.timed_out);
        assert_eq!(out.stdout.trim(), "done");
    }

    #[test]
    fn run_reports_a_spawn_failure() {
        let out = run_with_timeout(
            Path::new("/nonexistent/helper"),
            &[],
            None,
            Duration::from_secs(1),
        );
        assert_eq!(out.status, None);
        assert!(!out.timed_out);
        assert!(!out.stderr.is_empty());
    }

    #[test]
    fn probe_takes_only_the_first_line() {
        let line = probe_first_line(
            &sh(),
            &["-c", "printf 'NOT_PASTEABLE\\nPASTEABLE\\n'"],
            Duration::from_secs(2),
        );
        assert_eq!(line.as_deref(), Some("NOT_PASTEABLE"));
        let none = probe_first_line(&sh(), &["-c", "exec sleep 2"], Duration::from_millis(50));
        assert_eq!(none, None);
    }

    #[test]
    fn arch_check_rejects_non_mach_o() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("script");
        std::fs::write(&file, "#!/bin/sh\n").unwrap();
        assert!(arch_mismatch(&file, "Test").is_some());
        assert!(arch_mismatch(&dir.path().join("missing"), "Test").is_none());
    }
}
