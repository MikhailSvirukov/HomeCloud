//! Integration tests exercising the public node runtime ([`serve`]) over real
//! loopback sockets, so accepting connections and reacting to shutdown are
//! driven end to end rather than through in-memory helpers.

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use core::time::Duration;

use serde_json::Value;
use storage_node::{serve, split_frames, start_logging, Listener, LogConfig, Logger};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

fn max_frame_len() -> NonZeroU32 {
    NonZeroU32::new(64 * 1024).expect("test limit is non-zero")
}

fn queue_capacity() -> NonZeroUsize {
    NonZeroUsize::new(16).expect("test capacity is non-zero")
}

#[tokio::test]
async fn serve_stops_when_shutdown_resolves() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server = tokio::spawn(serve(
        listener,
        async move {
            let _ = shutdown_rx.await;
        },
        max_frame_len(),
        queue_capacity(),
        None,
    ));

    shutdown_tx
        .send(())
        .expect("the shutdown receiver stays alive until serve stops");

    let outcome = server.await;
    assert!(
        matches!(outcome, Ok(Ok(()))),
        "serve returns Ok when shutdown resolves"
    );
}

#[tokio::test]
async fn serve_accepts_and_closes_a_client_connection_on_shutdown() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server = tokio::spawn(serve(
        listener,
        async move {
            let _ = shutdown_rx.await;
        },
        max_frame_len(),
        queue_capacity(),
        None,
    ));

    let client = TcpStream::connect(address)
        .await
        .expect("connecting to the bound loopback address cannot fail");
    let (mut client_reader, mut client_writer) = split_frames(client, max_frame_len());
    client_writer
        .send_frame(b"ping")
        .await
        .expect("the accepted connection accepts the client frame");

    // Let `serve` observe the queued connection and spawn its pump before the
    // shutdown resolves, so the accepted connection is what gets closed.
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }

    shutdown_tx
        .send(())
        .expect("the shutdown receiver stays alive until serve stops");

    let outcome = server.await;
    assert!(
        matches!(outcome, Ok(Ok(()))),
        "serve returns Ok when shutdown resolves"
    );

    let closed = client_reader
        .next_frame()
        .await
        .expect("reading from the client connection cannot fail");
    assert_eq!(
        closed, None,
        "shutdown must close the connection accepted from the client"
    );
}

#[tokio::test]
async fn serve_logs_a_rejected_frame_with_peer_and_reason() {
    let directory = tempfile::TempDir::new().expect("creating a temporary directory cannot fail");
    let log_path = directory.path().join("events.jsonl");
    let config = LogConfig::new(
        log_path.clone(),
        NonZeroUsize::new(16).expect("log queue capacity is non-zero"),
        NonZeroU64::new(4096).expect("log file limit is non-zero"),
        NonZeroU64::new(65536).expect("log total limit is non-zero"),
        NonZeroU64::new(60).expect("log file age is non-zero"),
    )
    .expect("the log config limits are ordered");
    let logging = start_logging(config).expect("starting structured logging cannot fail");
    let logger: Logger = logging.logger().clone();

    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server = tokio::spawn(serve(
        listener,
        async move {
            let _ = shutdown_rx.await;
        },
        max_frame_len(),
        queue_capacity(),
        Some(logger.clone()),
    ));

    let mut client = TcpStream::connect(address)
        .await
        .expect("connecting to the bound loopback address cannot fail");
    client
        .write_all(&u32::MAX.to_be_bytes())
        .await
        .expect("the accepted connection accepts the frame length prefix");

    // The pump logs the rejection and then drops both socket halves, so the
    // client observes EOF only after the structured event has been queued.
    let mut eof = [0_u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(5), client.read(&mut eof))
        .await
        .expect("the rejected connection closes in time")
        .expect("the client can observe the close");
    assert_eq!(read, 0, "the server closes the rejected connection");

    shutdown_tx
        .send(())
        .expect("the shutdown receiver stays alive until serve stops");
    let outcome = server.await;
    assert!(
        matches!(outcome, Ok(Ok(()))),
        "serve returns Ok when shutdown resolves"
    );

    drop(logger);
    logging.shutdown().await;

    let contents = std::fs::read_to_string(&log_path).expect("the log file is readable");
    let rejected = contents
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("every log line is JSON"))
        .find(|record| record["event_type"] == "connection_rejected")
        .expect("the rejected frame is logged as a connection rejection");

    let peer_id = rejected["peer_id"]
        .as_str()
        .expect("a rejected connection records its peer");
    assert!(
        !peer_id.is_empty(),
        "the recorded peer identifier is not empty"
    );
    assert_eq!(rejected["error_category"], "invalid_request");
    assert_eq!(rejected["outcome"], "rejected");
    assert!(
        rejected.get("request_id").is_none(),
        "a rejection before request parsing has no request id"
    );
    for key in ["payload", "file_name", "login"] {
        assert!(
            rejected.get(key).is_none(),
            "the rejection record excludes the {key} field"
        );
    }
}
