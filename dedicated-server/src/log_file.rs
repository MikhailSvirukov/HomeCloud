//! Rotating structured log file with age, size and retention limits.
//!
//! The active file is rotated before a write that would exceed the configured
//! single-file size or after it has lived longer than the configured maximum
//! age, and at open when an existing active file already exceeds either limit.
//! A rotated file is named `<file name>.<UTC epoch milliseconds>`, with a
//! numeric suffix added when that name is taken, so rotation never overwrites
//! an existing file. Rotated files are pruned at open, after every rotation,
//! before a record that would cross the total size limit, and periodically
//! while records are written. Within one process rotation timestamps increase
//! monotonically, so a freed name is not reused there. A restart can reuse a
//! timestamp; the numeric suffix then separates the generations. Cross-restart
//! ordering is best effort when a reused timestamp and a previous-run file
//! share a modification time, because the suffix does not record which run is
//! newer.

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
    rotated_total: u64,
    segment_started_at: u64,
    last_rotation_millis: Option<u64>,
    last_prune_at: u64,
    now: C,
}

impl RotatingFile<fn() -> u64> {
    /// Open the configured active file, rotate it when it already exceeds a
    /// limit, and prune the rotated set.
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
            rotated_total: 0,
            segment_started_at,
            last_rotation_millis: None,
            last_prune_at: current,
            now,
        };
        if writer.should_rotate(current, 0) {
            writer.rotate(current)?;
        } else {
            writer.prune(current, 0)?;
        }
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

        let rotation_millis = self.next_rotation_millis();
        let rotated = self.rotated_path(rotation_millis)?;
        fs::rename(&self.path, &rotated)?;

        self.file = Some(open_append(&self.path)?);
        self.rotated_total = self.rotated_total.saturating_add(self.size);
        self.size = 0;
        self.segment_started_at = current;
        self.last_rotation_millis = Some(rotation_millis);

        self.prune(current, 0)
    }

    /// Next rotation timestamp: never below the segment start and strictly
    /// above the previous rotation, so names rise monotonically within a run.
    fn next_rotation_millis(&self) -> u64 {
        self.last_rotation_millis
            .map_or(self.segment_started_at, |last| {
                self.segment_started_at.max(last.saturating_add(1))
            })
    }

    fn prune_interval(&self) -> u64 {
        duration_millis(self.limits.retention).min(duration_millis(self.limits.max_file_age))
    }

    /// Delete rotated files, oldest first, until no file is expired and the
    /// retained total plus `incoming` bytes fits the total size limit. The
    /// active file is never removed.
    fn prune(&mut self, current: u64, incoming: usize) -> io::Result<()> {
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
            let Some((rotated_millis, generation)) = rotated_name_parts(&entry.file_name(), base)
            else {
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
                modified: metadata.modified()?,
                rotated_millis,
                generation,
            });
        }

        rotated.sort_by(|left, right| {
            left.modified
                .cmp(&right.modified)
                .then(left.rotated_millis.cmp(&right.rotated_millis))
                .then(left.generation.cmp(&right.generation))
                .then_with(|| left.path.cmp(&right.path))
        });

        let incoming = u64::try_from(incoming).unwrap_or(u64::MAX);
        let mut total_size = self.size.saturating_add(
            rotated
                .iter()
                .fold(0_u64, |total, file| total.saturating_add(file.size)),
        );
        let retention = duration_millis(self.limits.retention);

        for file in rotated {
            let expired = current.saturating_sub(millis_since_epoch(file.modified)) > retention;
            let fits = total_size.saturating_add(incoming) <= self.limits.max_total_size;
            if !expired && fits {
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
        self.rotated_total = total_size.saturating_sub(self.size);
        self.last_prune_at = current;

        Ok(())
    }

    fn rotated_path(&self, rotation_millis: u64) -> io::Result<PathBuf> {
        let base = self.path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "log path has no file name")
        })?;
        let mut suffix = None;
        let mut candidate = self
            .path
            .with_file_name(rotated_name(base, rotation_millis, suffix));
        while candidate.try_exists()? {
            suffix = Some(suffix.map_or(1, |value: u64| value.saturating_add(1)));
            candidate = self
                .path
                .with_file_name(rotated_name(base, rotation_millis, suffix));
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

        // Reserve the incoming record before appending it, so a crossing
        // record never needs filesystem space beyond the configured total.
        let incoming = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        let prune_due = current.saturating_sub(self.last_prune_at) >= self.prune_interval();
        let would_exceed = self
            .size
            .saturating_add(self.rotated_total)
            .saturating_add(incoming)
            > self.limits.max_total_size;
        if prune_due || would_exceed {
            self.prune(current, buf.len())?;
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
    modified: SystemTime,
    rotated_millis: u64,
    generation: u64,
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

/// Parse the rotation timestamp and numeric generation from a file name that
/// matches this path's rotated-name pattern; unrelated names return `None`.
///
/// A name without a suffix is generation zero, so it sorts before `.1`.
fn rotated_name_parts(file_name: &OsStr, base: &OsStr) -> Option<(u64, u64)> {
    let file_name = file_name.to_str()?;
    let base = base.to_str()?;
    let remainder = file_name.strip_prefix(base)?.strip_prefix('.')?;
    let (millis, generation) = match remainder.split_once('.') {
        Some((millis, generation)) => (millis, Some(generation)),
        None => (remainder, None),
    };
    if !is_ascii_digits(millis) {
        return None;
    }
    let generation = match generation {
        Some(generation) if is_ascii_digits(generation) => generation.parse().ok()?,
        Some(_) => return None,
        None => 0,
    };

    Some((millis.parse().ok()?, generation))
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
                    .is_some_and(|name| rotated_name_parts(name, OsStr::new(ACTIVE_NAME)).is_some())
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

        {
            let mut file = writer(&path, limits(50, 1_000, 1_000_000, 1_000_000), &clock);
            for index in 0..=4 {
                write_record(&mut file, index, 0);
            }
            file.flush().expect("the active file must flush");
        }

        let created = rotated_files(&dir.path);
        assert_eq!(created.len(), 4, "four files must have been rotated");
        for (offset, rotated) in created.iter().enumerate() {
            set_modified(rotated, BASE_MILLIS + offset as u64);
        }

        let _file = writer(&path, limits(50, 130, 1_000_000, 1_000_000), &clock);

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
    fn retention_expires_between_rotations() {
        let dir = TempDir::new("retention-interval");
        let path = dir.file(ACTIVE_NAME);
        let expired = dir.file(&format!("{ACTIVE_NAME}.{}", BASE_MILLIS - 500));
        fs::write(&expired, record(0, 0)).expect("the rotated file must be written");
        set_modified(&expired, BASE_MILLIS - 500);

        let clock = Arc::new(TestClock::new());
        let mut file = writer(
            &path,
            limits(1_000_000, 1_000_000, 1_000, 1_000_000),
            &clock,
        );
        assert!(
            expired.exists(),
            "the rotated file is not expired when the writer opens"
        );

        clock.advance(1_001);
        write_record(&mut file, 1, 0);
        file.flush().expect("the active file must flush");

        assert!(
            !expired.exists(),
            "retention must delete the rotated file without a rotation"
        );
        assert_eq!(rotated_files(&dir.path).len(), 0);
    }

    #[test]
    fn reopening_rotates_an_active_file_that_exceeds_the_size_limit() {
        let dir = TempDir::new("open-size");
        let path = dir.file(ACTIVE_NAME);
        let existing = record(0, 20);
        fs::write(&path, &existing).expect("the active file must be written");
        let clock = Arc::new(TestClock::new());

        let _file = writer(&path, limits(40, 1_000_000, 1_000_000, 1_000_000), &clock);

        let rotated = rotated_files(&dir.path);
        assert_eq!(rotated.len(), 1, "the oversized active file must rotate");
        assert_eq!(
            fs::read(&rotated[0]).expect("the rotated file must be readable"),
            existing
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("the active file must have metadata")
                .len(),
            0
        );
    }

    #[test]
    fn reopening_rotates_an_active_file_older_than_the_age_limit() {
        let dir = TempDir::new("open-age");
        let path = dir.file(ACTIVE_NAME);
        let existing = record(0, 0);
        fs::write(&path, &existing).expect("the active file must be written");
        set_modified(&path, BASE_MILLIS - 2_000);
        let clock = Arc::new(TestClock::new());

        let _file = writer(
            &path,
            limits(1_000_000, 1_000_000, 1_000_000, 1_000),
            &clock,
        );

        let rotated = rotated_files(&dir.path);
        assert_eq!(rotated.len(), 1, "the old active file must rotate");
        assert_eq!(
            fs::read(&rotated[0]).expect("the rotated file must be readable"),
            existing
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("the active file must have metadata")
                .len(),
            0
        );
    }

    #[test]
    fn rotation_adds_a_suffix_when_the_timestamp_name_exists() {
        let dir = TempDir::new("name-collision");
        let path = dir.file(ACTIVE_NAME);
        let existing = dir.file(&format!("{ACTIVE_NAME}.{BASE_MILLIS}"));
        fs::write(&existing, b"previous run").expect("the existing file must be written");
        let content = record(0, 20);
        fs::write(&path, &content).expect("the active file must be written");
        set_modified(&path, BASE_MILLIS);
        let clock = Arc::new(TestClock::new());

        let _file = writer(&path, limits(40, 1_000_000, 1_000_000, 1_000_000), &clock);

        assert_eq!(
            fs::read_to_string(&existing).expect("the existing file must be readable"),
            "previous run",
            "an existing rotated file must never be overwritten"
        );
        let renamed = dir.file(&format!("{ACTIVE_NAME}.{BASE_MILLIS}.1"));
        assert_eq!(
            fs::read(&renamed).expect("the suffixed rotated file must be readable"),
            content
        );
    }

    #[test]
    fn a_freed_rotation_name_is_not_reused_within_a_run() {
        let dir = TempDir::new("name-reuse");
        let path = dir.file(ACTIVE_NAME);
        let clock = Arc::new(TestClock::new());
        let mut file = writer(&path, limits(50, 1_000_000, 1, 1_000_000), &clock);

        write_record(&mut file, 0, 0);
        write_record(&mut file, 1, 0);
        let first = rotated_files(&dir.path);
        assert_eq!(first.len(), 1);
        set_modified(&first[0], BASE_MILLIS - 2);

        write_record(&mut file, 2, 0);
        write_record(&mut file, 3, 0);
        file.flush().expect("the active file must flush");

        let names: Vec<String> = rotated_files(&dir.path)
            .iter()
            .map(|path| {
                path.file_name()
                    .and_then(OsStr::to_str)
                    .expect("the rotated name must be UTF-8")
                    .to_owned()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                format!("{ACTIVE_NAME}.{}", BASE_MILLIS + 1),
                format!("{ACTIVE_NAME}.{}", BASE_MILLIS + 2),
            ],
            "the freed base name must not be reused within a run"
        );
    }

    #[test]
    fn equal_modification_times_prune_by_numeric_generation() {
        let dir = TempDir::new("generation-order");
        let path = dir.file(ACTIVE_NAME);
        let generation_two = dir.file(&format!("{ACTIVE_NAME}.{BASE_MILLIS}.2"));
        let generation_ten = dir.file(&format!("{ACTIVE_NAME}.{BASE_MILLIS}.10"));
        fs::write(&generation_two, vec![b'a'; 60]).expect("the rotated file must be written");
        fs::write(&generation_ten, vec![b'b'; 60]).expect("the rotated file must be written");
        set_modified(&generation_two, BASE_MILLIS);
        set_modified(&generation_ten, BASE_MILLIS);
        let clock = Arc::new(TestClock::new());

        let _file = writer(&path, limits(64, 100, 1_000_000, 1_000_000), &clock);

        assert!(
            !generation_two.exists(),
            "generation 2 must be removed before generation 10"
        );
        assert!(
            generation_ten.exists(),
            "generation 10 must survive the tie-break"
        );
    }

    #[test]
    fn an_oversized_record_is_written_whole_past_both_limits() {
        let dir = TempDir::new("oversized-record");
        let path = dir.file(ACTIVE_NAME);
        let clock = Arc::new(TestClock::new());
        let mut file = writer(&path, limits(10, 20, 1_000_000, 1_000_000), &clock);
        let oversized = record(0, 0);

        file.write_all(&oversized)
            .expect("the oversized record must be written whole");
        file.flush().expect("the active file must flush");

        let contents = fs::read_to_string(&path).expect("the active file must be readable");
        assert_eq!(contents.as_bytes(), oversized.as_slice());
        let value: serde_json::Value = serde_json::from_str(contents.trim_end())
            .expect("the oversized record must stay one complete JSON line");
        assert_eq!(value["index"], 0);
        assert_eq!(rotated_files(&dir.path).len(), 0);
        assert!(
            fs::metadata(&path)
                .expect("the active file must have metadata")
                .len()
                > 20,
            "the documented consequence is that the set exceeds the total limit"
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
