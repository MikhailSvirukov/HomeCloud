//! Structured JSON Lines logging with a bounded, non-blocking writer.
//!
//! SRS: `NFR-OBS-001 / 1`, `NFR-OBS-002 / 1`, `NFR-OBS-003 / 1`,
//! `NFR-OBS-004 / 1`, `CFG-OBS-001 / 1`, `CFG-OBS-002 / 1`.

use core::num::{NonZeroU64, NonZeroUsize};
use core::time::Duration;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Component name attached to every structured event.
pub const COMPONENT_NAME: &str = "storage-node";

/// Line terminator that makes every serialized event one JSON Lines record.
const LINE_TERMINATOR: u8 = b'\n';
/// Separator between the log path and a rotation suffix.
const ROTATION_SEPARATOR: &str = ".";

/// Validated structured logging configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    pub path: PathBuf,
    pub queue_capacity: NonZeroUsize,
    pub max_file_bytes: NonZeroU64,
    pub max_total_bytes: NonZeroU64,
    pub max_file_age: Duration,
}

impl LogConfig {
    /// Validates a structured logging configuration.
    pub fn new(
        path: PathBuf,
        queue_capacity: NonZeroUsize,
        max_file_bytes: NonZeroU64,
        max_total_bytes: NonZeroU64,
        max_file_age_seconds: NonZeroU64,
    ) -> Result<Self, LogConfigError> {
        if path.as_os_str().is_empty() {
            return Err(LogConfigError::EmptyPath);
        }
        if max_total_bytes < max_file_bytes {
            return Err(LogConfigError::TotalSmallerThanFile);
        }
        Ok(Self {
            path,
            queue_capacity,
            max_file_bytes,
            max_total_bytes,
            max_file_age: Duration::from_secs(max_file_age_seconds.get()),
        })
    }
}

/// Reasons a [`LogConfig`] cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LogConfigError {
    /// The log file path was empty.
    #[error("the log file path is empty")]
    EmptyPath,
    /// The total size limit cannot hold one full rotated file.
    #[error("the total log size limit is smaller than the per-file limit")]
    TotalSmallerThanFile,
}

/// Severity of a structured event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Kind of structured event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    StateChange,
    RequestAccepted,
    RequestFinished,
    ConnectionRejected,
}

/// Logical operation an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    Upload,
    CatalogPage,
    Registration,
    Heartbeat,
    RaftRpc,
    Other,
}

/// Result of a logical request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
    Rejected,
}

/// Safe error category that never carries user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Authentication,
    InvalidRequest,
    QuorumUnavailable,
    Timeout,
    Storage,
    Internal,
}

/// Significant lifecycle or availability transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChange {
    Started,
    Stopped,
    LayoutReady,
    ListenerBound,
    MetricsBound,
    DependencyUnavailable,
    QuorumUnavailable,
    StorageThresholdExceeded,
}

/// One structured JSON Lines record.
#[derive(Debug, Clone, Serialize)]
pub struct LogEvent {
    pub timestamp_ms: u64,
    pub component: &'static str,
    pub level: LogLevel,
    pub event_type: EventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_type: Option<OperationType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<ErrorCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_change: Option<StateChange>,
}

impl LogEvent {
    /// Builds an event that records a lifecycle or availability transition.
    pub fn state_change(level: LogLevel, change: StateChange) -> Self {
        Self {
            state_change: Some(change),
            ..Self::empty(level, EventType::StateChange)
        }
    }

    /// Builds the event emitted when a logical request enters processing.
    pub fn request_accepted(
        operation: OperationType,
        request_id: &str,
        peer_id: Option<&str>,
    ) -> Self {
        Self {
            operation_type: Some(operation),
            request_id: Some(request_id.to_owned()),
            peer_id: peer_id.map(str::to_owned),
            ..Self::empty(LogLevel::Info, EventType::RequestAccepted)
        }
    }

    /// Builds the event emitted when a logical request completes or fails.
    pub fn request_finished(
        operation: OperationType,
        request_id: &str,
        outcome: Outcome,
        duration_ms: u64,
        error_code: Option<u32>,
        error_category: Option<ErrorCategory>,
    ) -> Self {
        let level = match outcome {
            Outcome::Success => LogLevel::Info,
            Outcome::Failure => LogLevel::Error,
            Outcome::Rejected => LogLevel::Warn,
        };
        Self {
            operation_type: Some(operation),
            request_id: Some(request_id.to_owned()),
            outcome: Some(outcome),
            error_code,
            error_category,
            duration_ms: Some(duration_ms),
            ..Self::empty(level, EventType::RequestFinished)
        }
    }

    /// Builds the event emitted when a connection or frame is rejected before its request ID is known.
    pub fn connection_rejected(peer_id: Option<&str>, error_category: ErrorCategory) -> Self {
        Self {
            peer_id: peer_id.map(str::to_owned),
            outcome: Some(Outcome::Rejected),
            error_category: Some(error_category),
            ..Self::empty(LogLevel::Warn, EventType::ConnectionRejected)
        }
    }

    /// Attaches an upload ID to an existing event.
    pub fn with_upload_id(mut self, upload_id: &str) -> Self {
        self.upload_id = Some(upload_id.to_owned());
        self
    }

    /// Builds an event with only the mandatory fields populated.
    fn empty(level: LogLevel, event_type: EventType) -> Self {
        Self {
            timestamp_ms: now_millis(),
            component: COMPONENT_NAME,
            level,
            event_type,
            operation_type: None,
            peer_id: None,
            request_id: None,
            upload_id: None,
            outcome: None,
            error_code: None,
            error_category: None,
            duration_ms: None,
            state_change: None,
        }
    }
}

/// Clonable handle that enqueues events without blocking the caller.
#[derive(Clone)]
pub struct Logger {
    sender: mpsc::Sender<LogEvent>,
    dropped: Arc<AtomicU64>,
    write_errors: Arc<AtomicU64>,
}

impl Logger {
    /// Enqueues an event, counting it as dropped when the bounded queue is full or closed.
    pub fn log(&self, event: LogEvent) {
        if self.sender.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Returns the number of events dropped because the queue was full or closed.
    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Returns the number of events the writer failed to serialize or persist.
    pub fn write_errors(&self) -> u64 {
        self.write_errors.load(Ordering::Relaxed)
    }
}

impl fmt::Debug for Logger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Logger")
            .field("dropped_events", &self.dropped_events())
            .field("write_errors", &self.write_errors())
            .finish_non_exhaustive()
    }
}

/// Running structured logger and its writer task.
pub struct Logging {
    logger: Logger,
    writer: JoinHandle<()>,
}

impl Logging {
    /// Returns a cloneable handle for emitting events.
    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    /// Stops accepting events and waits for the writer to drain the queue.
    pub async fn shutdown(self) {
        let Self { logger, writer } = self;
        drop(logger);
        let _ = writer.await;
    }
}

/// Creates the log file, starts the writer task, and returns its handle.
pub fn start_logging(config: LogConfig) -> io::Result<Logging> {
    if let Some(parent) = config.path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let (file, length) = open_current_file(&config.path)?;
    let (sender, receiver) = mpsc::channel(config.queue_capacity.get());
    let write_errors = Arc::new(AtomicU64::new(0));
    let writer = Writer {
        config,
        file,
        length,
        opened: Instant::now(),
        write_errors: Arc::clone(&write_errors),
    };
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| io::Error::other("structured logging requires a Tokio runtime"))?;
    let writer = runtime.spawn(writer.run(receiver));
    Ok(Logging {
        logger: Logger {
            sender,
            dropped: Arc::new(AtomicU64::new(0)),
            write_errors,
        },
        writer,
    })
}

/// Async side of the logger that owns the current file and its rotation state.
struct Writer {
    config: LogConfig,
    file: File,
    length: u64,
    opened: Instant,
    write_errors: Arc<AtomicU64>,
}

impl Writer {
    /// Drains events until every sender is dropped, counting persistence failures.
    async fn run(mut self, mut receiver: mpsc::Receiver<LogEvent>) {
        let current_age_expired = tokio::fs::metadata(&self.config.path)
            .await
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > self.config.max_file_age);
        if (self.length > self.config.max_file_bytes.get() || current_age_expired)
            && self.rotate().await.is_err()
        {
            self.write_errors.fetch_add(1, Ordering::Relaxed);
        }
        if self.prune().await.is_err() {
            self.write_errors.fetch_add(1, Ordering::Relaxed);
        }
        while let Some(event) = receiver.recv().await {
            let mut line = match serde_json::to_vec(&event) {
                Ok(line) => line,
                Err(_) => {
                    self.write_errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            line.push(LINE_TERMINATOR);
            if self.write_line(&line).await.is_err() {
                self.write_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Rotates when needed, then appends and flushes one serialized line.
    async fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        if line.len() as u64 > self.config.max_file_bytes.get() {
            return Err(io::Error::other(
                "log line exceeds the configured maximum file size",
            ));
        }
        if self.needs_rotation(line.len() as u64) {
            self.rotate().await?;
        }
        self.file.write_all(line).await?;
        self.file.flush().await?;
        self.length = self.length.saturating_add(line.len() as u64);
        Ok(())
    }

    /// Returns whether the next line would exceed the size limit or the file has aged out.
    fn needs_rotation(&self, line_len: u64) -> bool {
        // A line larger than the limit is rejected by `write_line`.
        let exceeds_size = self.length > 0
            && self.length.saturating_add(line_len) > self.config.max_file_bytes.get();
        exceeds_size || self.opened.elapsed() > self.config.max_file_age
    }

    /// Renames the current file, opens a fresh one, and prunes old rotations.
    async fn rotate(&mut self) -> io::Result<()> {
        self.file.flush().await?;
        let rotated = unique_rotated_path(&self.config.path).await?;
        tokio::fs::rename(&self.config.path, &rotated).await?;
        self.file = open_current_file_async(&self.config.path).await?;
        self.length = 0;
        self.opened = Instant::now();
        if self.prune().await.is_err() {
            self.write_errors.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Deletes aged rotations, then the oldest rotations until the total size fits.
    async fn prune(&self) -> io::Result<()> {
        let Some(current_name) = self.config.path.file_name() else {
            return Ok(());
        };
        let Some(directory) = log_directory(&self.config.path) else {
            return Ok(());
        };
        let mut rotated = Vec::new();
        let mut entries = tokio::fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            let Some(timestamp_ms) = rotated_timestamp_ms(&entry.file_name(), current_name) else {
                continue;
            };
            let metadata = entry.metadata().await?;
            if !metadata.is_file() {
                continue;
            }
            rotated.push(RotatedFile {
                path: entry.path(),
                timestamp_ms,
                len: metadata.len(),
            });
        }
        rotated.sort_by_key(|file| file.timestamp_ms);
        let now = now_millis();
        let rotated_budget = self
            .config
            .max_total_bytes
            .get()
            .saturating_sub(self.config.max_file_bytes.get());
        let mut total: u64 = 0;
        let mut kept = Vec::with_capacity(rotated.len());
        for file in rotated {
            let age = Duration::from_millis(now.saturating_sub(file.timestamp_ms));
            if age > self.config.max_file_age && tokio::fs::remove_file(&file.path).await.is_ok() {
                continue;
            }
            total = total.saturating_add(file.len);
            kept.push(file);
        }
        for file in kept {
            if total <= rotated_budget {
                break;
            }
            if tokio::fs::remove_file(&file.path).await.is_ok() {
                total = total.saturating_sub(file.len);
            }
        }
        Ok(())
    }
}

/// A rotated file whose size contributes to the total storage budget.
struct RotatedFile {
    path: PathBuf,
    timestamp_ms: u64,
    len: u64,
}

/// Opens the current log file in append/create mode and returns its initial length.
fn open_current_file(path: &Path) -> io::Result<(File, u64)> {
    let file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    let length = file.metadata()?.len();
    Ok((File::from_std(file), length))
}

/// Opens the current log file asynchronously after a rotation.
async fn open_current_file_async(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await
}

/// Returns the directory that contains `path`, using the current directory for a bare name.
fn log_directory(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Some(parent),
        Some(_) => Some(Path::new(".")),
        None => None,
    }
}

/// Builds an unused rotation path for the current millisecond, adding a counter on collision.
async fn unique_rotated_path(path: &Path) -> io::Result<PathBuf> {
    let timestamp_ms = now_millis();
    let candidate = rotated_path(path, timestamp_ms, None);
    if !path_exists(&candidate).await {
        return Ok(candidate);
    }
    for counter in 1..=u32::MAX {
        let candidate = rotated_path(path, timestamp_ms, Some(counter));
        if !path_exists(&candidate).await {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no available rotated log file name",
    ))
}

/// Builds `<path>.<timestamp_ms>` or `<path>.<timestamp_ms>.<counter>`.
fn rotated_path(path: &Path, timestamp_ms: u64, counter: Option<u32>) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(ROTATION_SEPARATOR);
    name.push(timestamp_ms.to_string());
    if let Some(counter) = counter {
        name.push(ROTATION_SEPARATOR);
        name.push(counter.to_string());
    }
    PathBuf::from(name)
}

/// Returns the rotation timestamp encoded in a rotated file name.
fn rotated_timestamp_ms(name: &OsStr, current_name: &OsStr) -> Option<u64> {
    let name = name.to_str()?;
    let current_name = current_name.to_str()?;
    let suffix = name
        .strip_prefix(current_name)?
        .strip_prefix(ROTATION_SEPARATOR)?;
    let (timestamp, counter) = suffix
        .split_once(ROTATION_SEPARATOR)
        .unwrap_or((suffix, ""));
    if counter.is_empty() && suffix == timestamp && !timestamp.is_empty() {
        return timestamp.parse().ok();
    }
    let is_counter = !counter.is_empty() && counter.bytes().all(|byte| byte.is_ascii_digit());
    if is_counter {
        timestamp.parse().ok()
    } else {
        None
    }
}

/// Returns whether `path` exists.
async fn path_exists(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok()
}

/// Returns the current UTC time in milliseconds, or zero if the clock is unavailable.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    use tokio::sync::mpsc;

    use super::{LogEvent, LogLevel, Logger, StateChange};

    #[tokio::test]
    async fn log_counts_events_dropped_by_a_full_queue() {
        let (sender, _receiver) = mpsc::channel(1);
        let logger = Logger {
            sender,
            dropped: Arc::new(AtomicU64::new(0)),
            write_errors: Arc::new(AtomicU64::new(0)),
        };

        logger.log(LogEvent::state_change(LogLevel::Info, StateChange::Started));
        logger.log(LogEvent::state_change(LogLevel::Info, StateChange::Stopped));

        assert_eq!(logger.dropped_events(), 1);
    }
}
