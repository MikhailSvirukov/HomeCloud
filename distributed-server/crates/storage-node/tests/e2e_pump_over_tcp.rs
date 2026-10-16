//! End-to-end test driving the public frame pump over a real loopback TCP
//! connection: a [`Listener`]-accepted server half feeds [`pump_halves`], and a
//! plain client connection exercises both forwarding directions over the wire.

use storage_node::{encode_frame, pump_halves, split_frames, Listener};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Encodes `payload`, asserting the test payload is within the frame limit.
fn frame(payload: &[u8]) -> Vec<u8> {
    encode_frame(payload).expect("test payload is within the frame limit")
}

#[tokio::test]
async fn pump_forwards_frames_both_ways_over_tcp() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");

    let (inbound_tx, mut inbound_rx) = mpsc::channel(4);
    let (outbound_tx, outbound_rx) = mpsc::channel(4);

    let server = tokio::spawn(async move {
        let (reader, writer, _peer) = listener
            .accept()
            .await
            .expect("the listener accepts the client connection");
        pump_halves(reader, writer, inbound_tx, outbound_rx).await
    });

    let client = TcpStream::connect(address)
        .await
        .expect("connecting to the bound loopback address cannot fail");
    let (mut client_reader, mut client_writer) = split_frames(client);

    client_writer
        .send_frame(b"to-server")
        .await
        .expect("writing to the client connection cannot fail");
    let forwarded_to_server = inbound_rx
        .recv()
        .await
        .expect("the pump forwards the client frame before closing");
    assert_eq!(forwarded_to_server, frame(b"to-server"));

    outbound_tx
        .send(b"to-client".to_vec())
        .await
        .expect("the pump keeps the outbound channel open");
    let forwarded_to_client = client_reader
        .next_frame()
        .await
        .expect("reading from the client connection cannot fail")
        .expect("the pump wrote the outbound payload");
    assert_eq!(forwarded_to_client, frame(b"to-client"));

    drop(outbound_tx);
    assert!(
        matches!(server.await, Ok(Ok(()))),
        "the pump returns Ok when the outbound channel closes"
    );
}
