//! Integration tests exercising the public node runtime ([`serve`]) over real
//! loopback sockets, so accepting connections and reacting to shutdown are
//! driven end to end rather than through in-memory helpers.

use storage_node::{serve, split_frames, Listener};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

#[tokio::test]
async fn serve_stops_when_shutdown_resolves() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server = tokio::spawn(serve(listener, async move {
        let _ = shutdown_rx.await;
    }));

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
async fn serve_accepts_a_client_connection() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");

    let server = tokio::spawn(serve(listener, std::future::pending::<()>()));

    let client = TcpStream::connect(address)
        .await
        .expect("connecting to the bound loopback address cannot fail");
    let (_client_reader, mut client_writer) = split_frames(client);
    client_writer
        .send_frame(b"ping")
        .await
        .expect("the accepted connection accepts the client frame");

    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(
        !server.is_finished(),
        "serve keeps running until its shutdown resolves"
    );

    server.abort();
}
