//! Structured JSON Lines logging for the dedicated server.
//!
//! Every record is one UTF-8 JSON object per line on stderr and always carries
//! the fixed component name and an event type. Events without an `event` field
//! are ignored, and a record is written only when it serializes completely.
//!
//! Call sites may pass only values permitted by `NFR-OBS-003` into the known
//! field names; the formatter cannot tell a safe value from an unsafe one. In
//! particular, do not pass paths, file names, logins, keys, verifiers, payloads
//! or other user metadata.

use std::fmt;

use serde::Serialize;
use time::format_description::FormatItem;
use time::macros::format_description;
use tracing::field::{Field, Visit};
use tracing::{event, Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::{FormatTime, UtcTime};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

use crate::report::StartupError;

/// Component name written to every event.
const COMPONENT: &str = "dedicated-server";

/// Filter used when `RUST_LOG` is unset or cannot be parsed.
const DEFAULT_LOG_FILTER: &str = "info";

/// Event type for startup outcomes.
const EVENT_STARTUP: &str = "startup";

/// Event type for shutdown.
const EVENT_SHUTDOWN: &str = "shutdown";

/// Outcome value for a successful operation.
const OUTCOME_SUCCESS: &str = "success";

/// Outcome value for a failed operation.
const OUTCOME_FAILURE: &str = "failure";

/// Error code for a configuration loading failure.
const ERROR_CODE_CONFIGURATION: &str = "configuration_error";

/// Error code for an identity loading failure.
const ERROR_CODE_IDENTITY: &str = "identity_error";

/// Static message for a configuration loading failure.
const MESSAGE_CONFIGURATION: &str = "configuration loading failed";

/// Static message for an identity loading failure.
const MESSAGE_IDENTITY: &str = "key identity loading failed";

/// Static message for a normal shutdown.
const MESSAGE_SHUTDOWN: &str = "server stopped";

#[cfg(test)]
const FIELD_TIMESTAMP: &str = "timestamp";
#[cfg(test)]
const FIELD_COMPONENT: &str = "component";
#[cfg(test)]
const FIELD_LEVEL: &str = "level";
const FIELD_EVENT: &str = "event";
const FIELD_OPERATION: &str = "operation";
const FIELD_PEER_ID: &str = "peer_id";
const FIELD_OUTCOME: &str = "outcome";
const FIELD_ERROR_CODE: &str = "error_code";
const FIELD_DURATION_MS: &str = "duration_ms";
const FIELD_MESSAGE: &str = "message";

/// UTC timestamp format: RFC 3339 with exactly three fractional digits.
const TIMESTAMP_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

/// One structured log event as written to the log stream.
pub struct LogEvent {
    /// UTC time in RFC 3339 format with milliseconds.
    pub timestamp: String,
    /// Component that emitted the event.
    pub component: String,
    /// Importance level in uppercase text.
    pub level: String,
    /// Event type, such as `startup`.
    pub event: String,
    /// Operation type when the event belongs to an operation.
    pub operation: Option<String>,
    /// Peer identity when it has been established.
    pub peer_id: Option<String>,
    /// Operation outcome, either `success` or `failure`.
    pub outcome: Option<String>,
    /// Error code when the event reports a failure.
    pub error_code: Option<String>,
    /// Processing duration of a completion record.
    pub duration_ms: Option<u64>,
    /// Human-readable diagnostic text.
    pub message: Option<String>,
}

impl LogEvent {
    /// Render the event as one JSON line, omitting absent optional fields.
    pub fn to_json_line(&self) -> Result<String, serde_json::Error> {
        let encoded = EncodedEvent {
            timestamp: &self.timestamp,
            component: &self.component,
            level: &self.level,
            event: &self.event,
            operation: self.operation.as_deref(),
            peer_id: self.peer_id.as_deref(),
            outcome: self.outcome.as_deref(),
            error_code: self.error_code.as_deref(),
            duration_ms: self.duration_ms,
            message: self.message.as_deref(),
        };

        serde_json::to_string(&encoded)
    }
}

/// Serialization shape of [`LogEvent`], with the same key names.
#[derive(Serialize)]
struct EncodedEvent<'a> {
    timestamp: &'a str,
    component: &'a str,
    level: &'a str,
    event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

/// Event formatter that maps tracing events onto [`LogEvent`] records.
struct JsonFormatter {
    timer: UtcTime<&'static [FormatItem<'static>]>,
}

impl JsonFormatter {
    fn new() -> Self {
        Self {
            timer: UtcTime::new(TIMESTAMP_FORMAT),
        }
    }
}

impl<S, N> FormatEvent<S, N> for JsonFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let Some(event_type) = visitor.event else {
            return Ok(());
        };

        let mut timestamp = String::new();
        {
            let mut timestamp_writer = Writer::new(&mut timestamp);
            self.timer.format_time(&mut timestamp_writer)?;
        }

        let record = LogEvent {
            timestamp,
            component: COMPONENT.to_owned(),
            level: event.metadata().level().as_str().to_owned(),
            event: event_type,
            operation: visitor.operation,
            peer_id: visitor.peer_id,
            outcome: visitor.outcome,
            error_code: visitor.error_code,
            duration_ms: visitor.duration_ms,
            message: visitor.message,
        };

        let line = record.to_json_line().map_err(|_| fmt::Error)?;
        writer.write_str(&line)?;
        writer.write_char('\n')
    }
}

/// Collects the recognized event fields from a tracing event.
#[derive(Default)]
struct FieldVisitor {
    event: Option<String>,
    operation: Option<String>,
    peer_id: Option<String>,
    outcome: Option<String>,
    error_code: Option<String>,
    duration_ms: Option<u64>,
    message: Option<String>,
}

impl FieldVisitor {
    fn set(&mut self, field: &str, value: String) {
        match field {
            FIELD_EVENT => self.event = Some(value),
            FIELD_OPERATION => self.operation = Some(value),
            FIELD_PEER_ID => self.peer_id = Some(value),
            FIELD_OUTCOME => self.outcome = Some(value),
            FIELD_ERROR_CODE => self.error_code = Some(value),
            FIELD_MESSAGE => self.message = Some(value),
            _ => {}
        }
    }
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == FIELD_DURATION_MS {
            self.duration_ms = value.parse().ok();
        } else {
            self.set(field.name(), value.to_owned());
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == FIELD_DURATION_MS {
            self.duration_ms = Some(value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.set(field.name(), format!("{value:?}"));
    }
}

/// Install the JSON Lines subscriber, filtered by `RUST_LOG` and defaulting to `info`.
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));

    tracing_subscriber::fmt()
        .with_ansi(false)
        .event_format(JsonFormatter::new())
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

/// Log that the server started with its one-line summary.
pub fn startup(summary: &str) {
    event!(
        target: COMPONENT,
        Level::INFO,
        event = EVENT_STARTUP,
        outcome = OUTCOME_SUCCESS,
        message = %summary,
    );
}

/// Log that the server stopped.
pub fn shutdown() {
    event!(
        target: COMPONENT,
        Level::INFO,
        event = EVENT_SHUTDOWN,
        outcome = OUTCOME_SUCCESS,
        message = MESSAGE_SHUTDOWN,
    );
}

/// Log that startup failed, with the stage as the error code and a path-free message.
pub fn startup_failed(error: &StartupError) {
    let (error_code, message) = startup_failure(error);
    event!(
        target: COMPONENT,
        Level::ERROR,
        event = EVENT_STARTUP,
        outcome = OUTCOME_FAILURE,
        error_code = error_code,
        message = message,
    );
}

fn startup_failure(error: &StartupError) -> (&'static str, &'static str) {
    match error {
        StartupError::Configuration { .. } => (ERROR_CODE_CONFIGURATION, MESSAGE_CONFIGURATION),
        StartupError::Identity { .. } => (ERROR_CODE_IDENTITY, MESSAGE_IDENTITY),
    }
}

#[cfg(test)]
fn subscriber<W>(writer: W) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_ansi(false)
        .event_format(JsonFormatter::new())
        .with_max_level(Level::TRACE)
        .with_writer(writer)
        .finish()
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    /// Writer that keeps formatted output in memory for assertions.
    #[derive(Clone, Default)]
    struct CaptureWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl CaptureWriter {
        fn contents(&self) -> String {
            let bytes = self
                .bytes
                .lock()
                .expect("capture buffer must not be poisoned")
                .clone();
            String::from_utf8(bytes).expect("log output must be UTF-8")
        }
    }

    impl io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes
                .lock()
                .map_err(|_| io::Error::other("capture buffer poisoned"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for CaptureWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn parse(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("log line must be a JSON object")
    }

    fn timestamp_length() -> usize {
        "2026-10-17T12:34:56.789Z".len()
    }

    #[test]
    fn complete_event_serialises_to_one_json_line_with_every_field() {
        let event = LogEvent {
            timestamp: "2026-10-17T12:34:56.789Z".to_owned(),
            component: COMPONENT.to_owned(),
            level: "ERROR".to_owned(),
            event: EVENT_STARTUP.to_owned(),
            operation: Some("register".to_owned()),
            peer_id: Some("peer-1".to_owned()),
            outcome: Some(OUTCOME_FAILURE.to_owned()),
            error_code: Some(ERROR_CODE_IDENTITY.to_owned()),
            duration_ms: Some(42),
            message: Some("startup failed".to_owned()),
        };

        let line = event.to_json_line().expect("event must serialize");

        assert_eq!(line.lines().count(), 1);
        let value = parse(&line);
        assert_eq!(value[FIELD_TIMESTAMP], event.timestamp);
        assert_eq!(value[FIELD_COMPONENT], event.component);
        assert_eq!(value[FIELD_LEVEL], event.level);
        assert_eq!(value[FIELD_EVENT], event.event);
        assert_eq!(value[FIELD_OPERATION], "register");
        assert_eq!(value[FIELD_PEER_ID], "peer-1");
        assert_eq!(value[FIELD_OUTCOME], OUTCOME_FAILURE);
        assert_eq!(value[FIELD_ERROR_CODE], ERROR_CODE_IDENTITY);
        assert_eq!(value[FIELD_DURATION_MS], 42);
        assert_eq!(value[FIELD_MESSAGE], "startup failed");
    }

    #[test]
    fn event_without_optional_fields_omits_their_keys() {
        let event = LogEvent {
            timestamp: "2026-10-17T12:34:56.789Z".to_owned(),
            component: COMPONENT.to_owned(),
            level: "INFO".to_owned(),
            event: EVENT_STARTUP.to_owned(),
            operation: None,
            peer_id: None,
            outcome: None,
            error_code: None,
            duration_ms: None,
            message: None,
        };

        let line = event.to_json_line().expect("event must serialize");

        assert_eq!(line.lines().count(), 1);
        let value = parse(&line);
        let object = value.as_object().expect("line must be a JSON object");
        assert_eq!(object.len(), 4);
        for key in [
            FIELD_OPERATION,
            FIELD_PEER_ID,
            FIELD_OUTCOME,
            FIELD_ERROR_CODE,
            FIELD_DURATION_MS,
            FIELD_MESSAGE,
        ] {
            assert!(!object.contains_key(key), "unexpected key {key}");
        }
    }

    #[test]
    fn message_with_a_newline_stays_on_one_json_line() {
        let event = LogEvent {
            timestamp: "2026-10-17T12:34:56.789Z".to_owned(),
            component: COMPONENT.to_owned(),
            level: "ERROR".to_owned(),
            event: EVENT_STARTUP.to_owned(),
            operation: None,
            peer_id: None,
            outcome: Some(OUTCOME_FAILURE.to_owned()),
            error_code: Some(ERROR_CODE_CONFIGURATION.to_owned()),
            duration_ms: None,
            message: Some("first line\n  caused by: second line".to_owned()),
        };

        let line = event.to_json_line().expect("event must serialize");

        assert_eq!(line.lines().count(), 1);
        assert_eq!(
            parse(&line)[FIELD_MESSAGE],
            "first line\n  caused by: second line"
        );
    }

    #[test]
    fn tracing_event_through_the_formatter_produces_one_json_line() {
        let capture = CaptureWriter::default();
        let subscriber = subscriber(capture.clone());

        tracing::subscriber::with_default(subscriber, || {
            startup("dedicated-server listening on 10.0.0.1:7000");
        });

        let output = capture.contents();
        assert_eq!(output.lines().count(), 1);
        let value = parse(output.trim_end());
        assert_eq!(value[FIELD_COMPONENT], COMPONENT);
        assert_eq!(value[FIELD_LEVEL], "INFO");
        assert_eq!(value[FIELD_EVENT], EVENT_STARTUP);
        assert_eq!(value[FIELD_OUTCOME], OUTCOME_SUCCESS);
        assert_eq!(
            value[FIELD_MESSAGE],
            "dedicated-server listening on 10.0.0.1:7000"
        );
        let timestamp = value[FIELD_TIMESTAMP]
            .as_str()
            .expect("timestamp must be a string");
        assert!(timestamp.ends_with('Z'));
        assert_eq!(timestamp.len(), timestamp_length());
    }
}
