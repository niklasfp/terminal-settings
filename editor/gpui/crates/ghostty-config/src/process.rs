//! Running `ghostty` and `osascript` with a time limit, finding the Ghostty binary, and a private
//! temp file for validation.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long Ghostty or osascript may take before it's killed.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// What a finished process returned.
#[derive(Debug)]
pub struct Output {
    pub success: bool,
    /// stdout followed by stderr, decoded lossily.
    pub text: String,
}

#[derive(Debug)]
pub enum RunError {
    /// The process couldn't be started.
    Start(io::Error),
    /// It didn't finish in time and was killed.
    TimedOut,
}

/// Runs `program` with no stdin, capturing stdout and stderr, and kills it after `timeout`.
pub fn run<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
    timeout: Duration,
) -> Result<Output, RunError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::Start)?;
    let deadline = Instant::now() + timeout;
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::TimedOut);
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Start(e));
            }
        }
    };
    // A grandchild could keep the pipes open after the process exits; don't wait past the deadline.
    let collect = |rx: mpsc::Receiver<Vec<u8>>| {
        let left = deadline.saturating_duration_since(Instant::now());
        rx.recv_timeout(left.max(Duration::from_millis(100)))
            .unwrap_or_default()
    };
    let mut bytes = collect(stdout);
    bytes.extend(collect(stderr));
    Ok(Output {
        success: status.success(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

fn read_in_background<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

/// The Ghostty CLI: $GHOSTTY_BIN, then the macOS app bundles, then PATH.
pub fn ghostty_binary() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("GHOSTTY_BIN").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Applications/Ghostty.app/Contents/MacOS/ghostty",
        ));
        if let Some(home) = std::env::home_dir() {
            candidates.push(home.join("Applications/Ghostty.app/Contents/MacOS/ghostty"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        let name = if cfg!(windows) {
            "ghostty.exe"
        } else {
            "ghostty"
        };
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(name)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// A new file in `dir` that nothing else could have created or linked first.
/// Removed when dropped.
pub struct PrivateTempFile {
    pub path: PathBuf,
}

impl PrivateTempFile {
    /// Creates the file exclusively (never opens an existing file or follows a link), owner-only.
    pub fn create(dir: &Path, prefix: &str, suffix: &str) -> io::Result<(Self, File)> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut last_error = None;
        for _ in 0..16 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(
                "{prefix}-{}-{nanos:x}-{n}{suffix}",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => return Ok((Self { path }, file)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last_error = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::other("no free temp file name")))
    }
}

impl Drop for PrivateTempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("fake");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn captures_stdout_then_stderr_and_the_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "echo out; echo err >&2; exit 3");
        let out = run(&bin, &[] as &[&str], TIMEOUT).unwrap();
        assert!(!out.success);
        assert_eq!(out.text, "out\nerr\n");
    }

    #[test]
    fn kills_a_process_that_takes_too_long() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "exec sleep 30");
        let started = Instant::now();
        let result = run(&bin, &[] as &[&str], Duration::from_millis(200));
        assert!(matches!(result, Err(RunError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_missing_program_is_a_start_error() {
        let result = run(Path::new("/nonexistent/ghostty"), &["x"], TIMEOUT);
        assert!(matches!(result, Err(RunError::Start(_))));
    }

    #[test]
    fn temp_file_is_new_private_and_removed_on_drop() {
        let (temp, mut file) =
            PrivateTempFile::create(&std::env::temp_dir(), "ghostty-settings-test", ".ghostty").unwrap();
        file.write_all(b"x").unwrap();
        let mode = fs::metadata(&temp.path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let path = temp.path.clone();
        drop(temp);
        assert!(!path.exists());
    }

    #[test]
    fn temp_file_never_opens_an_existing_path() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, "keep").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        // The same flags PrivateTempFile uses refuse both a symlink and a regular file.
        assert!(options.open(&link).is_err());
        assert!(options.open(&victim).is_err());
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
    }
}
