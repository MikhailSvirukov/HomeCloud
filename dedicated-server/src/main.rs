//! Binary entry point for the dedicated server control plane.
//!
//! Resolves the configuration path from the environment or the default file
//! name, loads the configuration to learn the structured log path, installs the
//! non-blocking file writer, and then loads the key identity. Startup failures
//! before the writer exists are written directly to stderr; later startup and
//! shutdown events go to the configured file.
//!
//! After a successful start the process serves the Prometheus metrics endpoint
//! and runs until `SIGINT` or `SIGTERM`, then writes the shutdown event and
//! exits. The application TCP protocol is added in later issues.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use dedicated_server::{
    init as init_logging, logging_failed_to_stderr, metrics_failed, runtime_failed,
    shutdown as log_shutdown, start_metrics_server, startup as log_startup,
    startup_failed as log_startup_failed, startup_failed_to_stderr, LogGuard, Metrics,
    ServerConfig, Startup, StartupError, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME,
};

/// How often the run loop checks the shutdown flag.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Process allocator tuned for the concurrent workload of a server process
/// rather than the system default.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    let path = config_path();
    let config = match ServerConfig::load(&path) {
        Ok(config) => config,
        Err(source) => {
            let error = StartupError::Configuration { path, source };
            let _ = startup_failed_to_stderr(&error);
            return ExitCode::FAILURE;
        }
    };

    let guard = match init_logging(config.logging()) {
        Ok(guard) => guard,
        Err(_) => {
            let _ = logging_failed_to_stderr();
            return ExitCode::FAILURE;
        }
    };
    // The guard stays reachable from the metrics endpoint and is flushed
    // explicitly at shutdown, before the process exits.
    let log = Arc::new(Mutex::new(Some(guard)));

    let startup = match Startup::from_config(config) {
        Ok(startup) => startup,
        Err(error) => {
            log_startup_failed(&error);
            return ExitCode::FAILURE;
        }
    };

    let metrics = Arc::new(Metrics::new(&startup.cluster_labels()));
    let body_metrics = Arc::clone(&metrics);
    let body_log = Arc::clone(&log);
    let body: Arc<dyn Fn() -> String + Send + Sync> = Arc::new(move || {
        let dropped_records = body_log
            .lock()
            .ok()
            .and_then(|slot| {
                slot.as_ref()
                    .map(dedicated_server::LogGuard::dropped_records)
            })
            .unwrap_or(0);
        body_metrics.render(dropped_records)
    });

    let _metrics = match start_metrics_server(startup.metrics_bind(), body) {
        Ok(server) => server,
        Err(_) => {
            metrics_failed();
            flush_log(&log);
            return ExitCode::FAILURE;
        }
    };

    log_startup(&startup.summary());

    let stop = match shutdown_flag() {
        Ok(stop) => stop,
        Err(_) => {
            runtime_failed();
            flush_log(&log);
            return ExitCode::FAILURE;
        }
    };
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(SHUTDOWN_POLL_INTERVAL);
    }

    log_shutdown();
    flush_log(&log);

    ExitCode::SUCCESS
}

/// Drop the log guard so the background writer flushes every buffered record.
fn flush_log(log: &Arc<Mutex<Option<LogGuard>>>) {
    if let Ok(mut slot) = log.lock() {
        drop(slot.take());
    }
}

/// Install `SIGINT` and `SIGTERM` handlers that set the shutdown flag.
fn shutdown_flag() -> std::io::Result<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;

    Ok(stop)
}

/// Resolve the configuration path from [`CONFIG_PATH_ENV`] or the default file name.
fn config_path() -> PathBuf {
    match env::var(CONFIG_PATH_ENV) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_CONFIG_FILE_NAME),
    }
}
