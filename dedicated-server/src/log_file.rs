//! Rotating structured log file with age, size and retention limits.
//!
//! The active file is rotated before a write that would exceed the configured
//! single-file size or after it has lived longer than the configured maximum
//! age. A rotated file is named `<file name>.<UTC epoch milliseconds>`, with a
//! numeric suffix added when that name is taken, so rotation never overwrites
//! an existing file. After every rotation the rotated set is pruned by age and
//! by the total size of the active file plus all rotated files.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Retention and rotation limits taken from the `[logging]` configuration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LogLimits {
    /// How long a rotated file is kept.
    pub(crate) retention: Duration,
    /// Maximum age of the active file before it is rotated.
    pub(crate) max_file_age: Duration,
    /// Maximum size of one log file in bytes.
    pub(crate) max_file_size: u64,
    /// Maximum total size of the active file plus every rotated file, in bytes.
    pub(crate) max_total_size: u64,
}

/// Current UTC time in milliseconds since the Unix epoch.
fn system_now_millis() -> u64 {
    millis_since_epoch(SystemTime::now())
}

fn millis_since_epoch(time: SystemTime) -> u64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Append-only active log file that rotates and prunes itself.
///
/// `C` is the clock seam: it returns UTC milliseconds since the Unix epoch and
/// exists so tests can advance time without sleeping.
pub(crate) struct RotatingFile<C = fn() -> u64> {
    path: PathBuf,
    limits: LogLimits,
    file: Option<File>,
    size: u64,
    segment_started_at: u64,
    now: C,
}

impl RotatingFile<fn() -> u64> {
    /// Open the configured active file and prune the rotated set.
    pub(crate) fn new(path: &Path, limits: LogLimits) -> io::Result<Self> {
        Self::open_with_clock(path, limits, system_now_millis)
    }
}

impl<C: Fn() -> u64> RotatingFile<C> {
    fn open_with_clock(path: &Path, limits: LogLimits, now: C) -> io::Result<Self> {
        let file = open_append(path)?;
        let metadata = file.metadata()?;
        let size = metadata.len();
        let current = now();
        let segment_started_at = if size == 0 {
            current
        } else {
            metadata
                .modified()
                .map(millis_since_epoch)
                .unwrap_or(current)
        };

        let mut writer = Self {
            path: path.to_path_buf(),
            limits,
            file: Some(file),
            size,
            segment_started_at,
            now,
        };
        writer.prune(current)?;
        Ok(writer)
    }

    fn should_rotate(&self, current: u64, incoming: usize) -> bool {
        let incoming = u64::try_from(incoming).unwrap_or(u64::MAX);
        // A record that alone exceeds the limit still goes to an empty active
        // file; splitting it would corrupt the JSON Lines stream.
        let exceeds_size =
            self.size > 0 && self.size.saturating_add(incoming) > self.limits.max_file_size;
        let exceeds_age = current.saturating_sub(self.segment_started_at)
            > duration_millis(self.limits.max_file_age);

        exceeds_size || exceeds_age
    }

    fn rotate(&mut self, current: u64) -> io::Result<()> {
        self.flush()?;
        drop(self.file.take());

        let rotated = self.rotated_path(current)?;
        fs::rename(&self.path, &rotated)?;

        self.file = Some(open_append(&self.path)?);
        self.size = 0;
        self.segment_started_at = current;

        self.prune(current)
    }

    /// Delete rotated files, oldest first, until neither the retention period
    /// nor the total size limit is exceeded. The active file is never removed.
    fn prune(&mut self, current: u64) -> io::Result<()> {
        let parent = match self.path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let base = self.path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "log path has no file name")
        })?;

        let mut rotated = Vec::new();
        for entry in fs::read_dir(parent)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let Some(rotated_millis) = rotated_millis(&entry.file_name(), base) else {
                continue;
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !metadata.is_file() {
                continue;
            }
            rotated.push(RotatedFile {
                path: entry.path(),
                size: metadata.len(),
                modified: metadata.modified().map(millis_since_epoch)?,
                rotated_millis,
            });
        }

        rotated.sort_by(|left, right| {
            left.modified
                .cmp(&right.modified)
                .then(left.rotated_millis.cmp(&right.rotated_millis))
                .then_with(|| left.path.cmp(&right.path))
        });

        let mut total_size = self.size.saturating_add(
            rotated
                .iter()
                .fold(0_u64, |total, file| total.saturating_add(file.size)),
        );
        let retention = duration_millis(self.limits.retention);

        for file in rotated {
            let expired = current.saturating_sub(file.modified) > retention;
            if !expired && total_size <= self.limits.max_total_size {
                break;
            }
            match fs::remove_file(&file.path) {
                Ok(()) => total_size = total_size.saturating_sub(file.size),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    total_size = total_size.saturating_sub(file.size);
                }
                Err(error) => return Err(error),
            }
        }

        Ok(())
    }

    fn rotated_path(&self, current: u64) -> io::Result<PathBuf> {
        let base = self.path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "log path has no file name")
        })?;
        let mut suffix = None;
        let mut candidate = self
            .path
            .with_file_name(rotated_name(base, current, suffix));
        while candidate.try_exists()? {
            suffix = Some(suffix.map_or(1, |value: u64| value.saturating_add(1)));
            candidate = self
                .path
                .with_file_name(rotated_name(base, current, suffix));
        }
        Ok(candidate)
    }

    fn file(&mut self) -> io::Result<&mut File> {
        if self.file.is_none() {
            self.file = Some(open_append(&self.path)?);
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file is not open"))
    }
}

impl<C: Fn() -> u64> Write for RotatingFile<C> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let current = (self.now)();
        if self.should_rotate(current, buf.len()) {
            self.rotate(current)?;
        }
        let written = self.file()?.write(buf)?;
        self.size = self
            .size
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file()?.flush()
    }
}

/// A rotated file found next to the active file.
struct RotatedFile {
    path: PathBuf,
    size: u64,
    modified: u64,
    rotated_millis: u64,
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn rotated_name(base: &OsStr, millis: u64, suffix: Option<u64>) -> PathBuf {
    let mut name = base.to_os_string();
    name.push(format!(".{millis}"));
    if let Some(suffix) = suffix {
        name.push(format!(".{suffix}"));
    }
    name.into()
}

/// Parse the rotation timestamp from a file name that matches this path's
/// rotated-name pattern; unrelated names return `None`.
fn rotated_millis(file_name: &OsStr, base: &OsStr) -> Option<u64> {
    let file_name = file_name.to_str()?;
    let base = base.to_str()?;
    let remainder = file_name.strip_prefix(base)?.strip_prefix('.')?;
    let (millis, suffix) = match remainder.split_once('.') {
        Some((millis, suffix)) => (millis, Some(suffix)),
        None => (remainder, None),
    };
    if !is_ascii_digits(millis) {
        return None;
    }
    if let Some(suffix) = suffix {
        if !is_ascii_digits(suffix) {
            return None;
        }
    }

    millis.parse().ok()
}

fn is_ascii_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use std::fs::{FileTimes, OpenOptions};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use super::*;

    const BASE_MILLIS: u64 = 1_700_000_000_000;
    const ACTIVE_NAME: &str = "dedicated-server.log";

    /// Owns a per-test temporary directory and removes it on drop.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dedicated-server-log-file-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("test directory must be created");
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// Clock that tests advance explicitly.
    struct TestClock {
        millis: AtomicU64,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                millis: AtomicU64::new(BASE_MILLIS),
            }
        }

        fn advance(&self, delta: u64) {
            self.millis.fetch_add(delta, Ordering::Relaxed);
        }

        fn millis(&self) -> u64 {
            self.millis.load(Ordering::Relaxed)
        }
    }

    fn limits(
        max_file_size: u64,
        max_total_size: u64,
        retention_ms: u64,
        max_file_age_ms: u64,
    ) -> LogLimits {
        LogLimits {
            retention: Duration::from_millis(retention_ms),
            max_file_age: Duration::from_millis(max_file_age_ms),
            max_file_size,
            max_total_size,
        }
    }

    fn writer(
        path: &Path,
        limits: LogLimits,
        clock: &Arc<TestClock>,
    ) -> RotatingFile<impl Fn() -> u64> {
        let clock = Arc::clone(clock);
        RotatingFile::open_with_clock(path, limits, move || clock.millis())
            .expect("the log file must open")
    }

    fn record(index: usize, padding: usize) -> Vec<u8> {
        format!(
            "{{\"event\":\"startup\",\"index\":{index},\"padding\":\"{}\"}}\n",
            "x".repeat(padding)
        )
        .into_bytes()
    }

    fn write_record(file: &mut impl Write, index: usize, padding: usize) {
        file.write_all(&record(index, padding))
            .expect("the record must be written");
    }

    fn rotated_files(dir: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .expect("the log directory must be readable")
            .map(|entry| entry.expect("the directory entry must be readable").path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| rotated_millis(name, OsStr::new(ACTIVE_NAME)).is_some())
            })
            .collect();
        files.sort();
        files
    }

    fn set_modified(path: &Path, millis: u64) {
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .expect("the rotated file must open");
        file.set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_millis(millis)))
            .expect("the modified time must be set");
    }

    #[test]
    fn writing_past_the_size_limit_rotates_with_the_complete_earlier_records() {
        let dir = TempDir::new("size-rotation");
        let path = dir.file(ACTIVE_NAME);
        let clock = Arc::new(TestClock::new());
        let mut file = writer(&path, limits(64, 1_000_000, 1_000_000, 1_000_000), &clock);

        write_record(&mut file, 0, 20);
        write_record(&mut file, 1, 20);
        file.flush().expect("the active file must flush");

        let rotated = rotated_files(&dir.path);
        assert_eq!(rotated.len(), 1, "one file must have been rotated");
        let rotated_contents =
            fs::read_to_string(&rotated[0]).expect("the rotated file must be readable");
        assert_eq!(
            rotated_contents.as_bytes(),
            record(0, 20).as_slice(),
            "the rotated file must keep the earlier record intact"
        );
        let active = fs::read_to_string(&path).expect("the active file must be readable");
        assert_eq!(active.as_bytes(), record(1, 20).as_slice());
    }

    #[test]
    fn age_past_the_limit_rotates_without_reaching_the_size_limit() {
        let dir = TempDir::new("age-rotation");
        let path = dir.file(ACTIVE_NAME);
        let clock = Arc::new(TestClock::new());
        let mut file = writer(
            &path,
            limits(1_000_000, 1_000_000, 1_000_000, 1_000),
            &clock,
        );

        write_record(&mut file, 0, 0);
        clock.advance(1_001);
        write_record(&mut file, 1, 0);
        file.flush().expect("the active file must flush");

        let rotated = rotated_files(&dir.path);
        assert_eq!(rotated.len(), 1);
        assert_eq!(
            rotated[0].file_name().and_then(OsStr::to_str),
            Some(format!("{ACTIVE_NAME}.{BASE_MILLIS}").as_str())
        );
        let rotated_contents =
            fs::read_to_string(&rotated[0]).expect("the rotated file must be readable");
        assert_eq!(rotated_contents.as_bytes(), record(0, 0).as_slice());
        let active = fs::read_to_string(&path).expect("the active file must be readable");
        assert_eq!(active.as_bytes(), record(1, 0).as_slice());
    }

    #[test]
    fn a_rotated_file_older_than_the_retention_period_is_pruned() {
        let dir = TempDir::new("retention");
        let path = dir.file(ACTIVE_NAME);
        let clock = Arc::new(TestClock::new());
        let mut file = writer(&path, limits(64, 1_000_000, 1_000, 1_000_000), &clock);

        write_record(&mut file, 0, 0);
        write_record(&mut file, 1, 0);
        let first = rotated_files(&dir.path);
        assert_eq!(first.len(), 1);
        set_modified(&first[0], BASE_MILLIS - 1_001);

        write_record(&mut file, 2, 0);
        file.flush().expect("the active file must flush");

        let remaining = rotated_files(&dir.path);
        assert_eq!(remaining.len(), 1, "the expired rotated file must be gone");
        assert_ne!(remaining[0], first[0]);
        assert!(!first[0].exists());
        assert!(path.exists(), "the active file must never be pruned");
    }

    #[test]
    fn pruning_removes_oldest_rotated_files_until_the_total_size_fits() {
        let dir = TempDir::new("total-size");
        let path = dir.file(ACTIVE_NAME);
        let clock = Arc::new(TestClock::new());
        let mut file = writer(&path, limits(40, 130, 1_000_000, 1_000_000), &clock);

        for index in 0..=4 {
            write_record(&mut file, index, 0);
        }
        file.flush().expect("the active file must flush");

        let remaining = rotated_files(&dir.path);
        assert_eq!(remaining.len(), 2, "only two rotated files fit");
        let contents: Vec<String> = remaining
            .iter()
            .map(|path| fs::read_to_string(path).expect("the rotated file must be readable"))
            .collect();
        assert!(
            contents.iter().all(|content| !content.contains("\"index\":0")
                && !content.contains("\"index\":1")),
            "the oldest rotated files must be removed: {contents:?}"
        );
        let total: u64 = remaining
            .iter()
            .map(|path| {
                fs::metadata(path)
                    .expect("the rotated file must have metadata")
                    .len()
            })
            .sum();
        assert!(total + fs::metadata(&path).expect("active metadata").len() <= 130);
        let active = fs::read_to_string(&path).expect("the active file must be readable");
        assert!(
            active.contains("\"index\":4"),
            "the active file must keep the newest record"
        );
    }

    #[test]
    fn unrelated_files_are_never_pruned() {
        let dir = TempDir::new("unrelated");
        let path = dir.file(ACTIVE_NAME);
        for name in [
            "notes.txt",
            "other.log.123",
            "dedicated-server.log.bak",
            "dedicated-server.log.123x",
        ] {
            fs::write(dir.file(name), b"keep me").expect("the unrelated file must be written");
        }
        let clock = Arc::new(TestClock::new());
        let mut file = writer(&path, limits(40, 100, 1_000_000, 1_000_000), &clock);

        for index in 0..=4 {
            write_record(&mut file, index, 0);
        }
        file.flush().expect("the active file must flush");

        for name in [
            "notes.txt",
            "other.log.123",
            "dedicated-server.log.bak",
            "dedicated-server.log.123x",
        ] {
            assert!(
                dir.file(name).exists(),
                "the unrelated file {name} must survive pruning"
            );
        }
    }

    #[test]
    fn every_rotated_file_contains_only_complete_json_lines() {
        let dir = TempDir::new("json-lines");
        let path = dir.file(ACTIVE_NAME);
        let clock = Arc::new(TestClock::new());
        let mut file = writer(&path, limits(120, 1_000_000, 1_000_000, 1_000_000), &clock);

        for index in 0..7 {
            write_record(&mut file, index, 10);
        }
        file.flush().expect("the active file must flush");

        let rotated = rotated_files(&dir.path);
        assert_eq!(rotated.len(), 3, "three files must have been rotated");
        for path in rotated.iter().chain(std::iter::once(&path)) {
            let contents = fs::read_to_string(path).expect("the log file must be readable");
            assert!(
                contents.lines().all(|line| !line.is_empty()),
                "a log file must not contain blank lines: {contents}"
            );
            for line in contents.lines() {
                let value: serde_json::Value =
                    serde_json::from_str(line).expect("every line must be complete JSON");
                assert!(value.get("index").is_some());
            }
        }
    }
}
