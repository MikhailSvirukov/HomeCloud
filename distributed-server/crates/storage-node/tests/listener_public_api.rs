//! Integration tests exercising the public TCP [`Listener`] over real loopback
//! sockets, so binding, address reporting, and accepted frame halves are driven
//! end to end rather than through in-memory buffer helpers.

use storage_node::{encode_frame, split_frames, Listener, ListenerError};
use tokio::net::TcpStream;

/// Encodes `payload`, asserting the test payload fits within the frame limit.
fn frame(payload: &[u8]) -> Vec<u8> {
    encode_frame(payload).expect("test payload is within the frame limit")
}

#[tokio::test]
async fn binds_an_ephemeral_port() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");

    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");

    assert_ne!(
        address.port(),
        0,
        "an ephemeral bind must report the concrete port the OS chose"
    );
}

#[tokio::test]
async fn accepts_a_connection_and_exchanges_frames() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");

    let accept_task = tokio::spawn(async move { listener.accept().await });

    let client = TcpStream::connect(address)
        .await
        .expect("connecting to the bound loopback address cannot fail");
    let (mut client_reader, mut client_writer) = split_frames(client);

    let (mut server_reader, mut server_writer, _peer) = accept_task
        .await
        .expect("the accept task does not panic")
        .expect("the listener accepts the client connection");

    server_writer
        .send_frame(b"from-server")
        .await
        .expect("writing to the accepted connection cannot fail");
    client_writer
        .send_frame(b"from-client")
        .await
        .expect("writing to the client connection cannot fail");

    let seen_by_client = client_reader
        .next_frame()
        .await
        .expect("reading from the client connection cannot fail")
        .expect("the server frame was written");
    let seen_by_server = server_reader
        .next_frame()
        .await
        .expect("reading from the accepted connection cannot fail")
        .expect("the client frame was written");

    assert_eq!(seen_by_client, frame(b"from-server"));
    assert_eq!(seen_by_server, frame(b"from-client"));
}

#[tokio::test]
async fn bind_rejects_an_invalid_address() {
    let outcome = Listener::bind("not-an-address").await;

    assert!(
        matches!(outcome, Err(ListenerError::InvalidAddr { .. })),
        "a malformed listen address must be reported as an invalid address"
    );
}
