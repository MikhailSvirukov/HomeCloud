//! Integration tests exercising the public frame reader/writer, `split_frames`,
//! and `pump_frames` over real duplex streams, so the connection halves are
//! driven end to end rather than through in-memory buffer helpers.

use storage_node::{
    encode_frame, pump_frames, split_frames, ConnectionError, FrameReader, FrameWriter,
    FramingError, MAX_FRAME_PAYLOAD_LEN,
};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;

/// Encodes `payload`, asserting the test payload is within the frame limit.
fn frame(payload: &[u8]) -> Vec<u8> {
    encode_frame(payload).expect("test payload is within the frame limit")
}

/// Builds a connected pair of duplex endpoints sharing `capacity` bytes.
fn duplex_pair(capacity: usize) -> (DuplexStream, DuplexStream) {
    duplex(capacity)
}

#[tokio::test]
async fn frames_round_trip_in_both_directions() {
    let (client, server) = duplex_pair(1024);
    let (mut client_reader, mut client_writer) = split_frames(client);
    let (mut server_reader, mut server_writer) = split_frames(server);

    client_writer
        .send_frame(b"ping")
        .await
        .expect("in-memory duplex write cannot fail");
    server_writer
        .send_frame(b"pong")
        .await
        .expect("in-memory duplex write cannot fail");

    let received_by_server = server_reader
        .next_frame()
        .await
        .expect("in-memory duplex read cannot fail")
        .expect("the client frame was written");
    let received_by_client = client_reader
        .next_frame()
        .await
        .expect("in-memory duplex read cannot fail")
        .expect("the server frame was written");

    assert_eq!(received_by_server, frame(b"ping"));
    assert_eq!(received_by_client, frame(b"pong"));
}

#[tokio::test]
async fn next_frame_returns_none_at_eof() {
    let (peer, reader_half) = duplex_pair(1024);
    let mut reader = FrameReader::new(reader_half);
    drop(peer);

    let received = reader
        .next_frame()
        .await
        .expect("read from a closed duplex cannot fail");

    assert!(received.is_none(), "end of stream must yield no frame");
}

#[tokio::test]
async fn frame_split_across_writes_is_reassembled() {
    let encoded = frame(b"split across writes");
    let midpoint = encoded.len() / 2;
    let (head, tail) = encoded.split_at(midpoint);

    let (mut raw_writer, reader_half) = duplex_pair(8);
    let mut reader = FrameReader::new(reader_half);

    let write_two_chunks = async {
        raw_writer
            .write_all(head)
            .await
            .expect("in-memory duplex write cannot fail");
        raw_writer
            .write_all(tail)
            .await
            .expect("in-memory duplex write cannot fail");
    };

    let ((), received) = tokio::join!(write_two_chunks, reader.next_frame());

    let received = received
        .expect("in-memory duplex read cannot fail")
        .expect("both halves form one frame");
    assert_eq!(received, encoded);
}

#[tokio::test]
async fn max_size_frame_followed_by_another_is_read_whole() {
    let big_payload = vec![0xAB; MAX_FRAME_PAYLOAD_LEN as usize];
    let big_frame = frame(&big_payload);
    let small_frame = frame(b"tail");

    let (client, server) = duplex_pair(64 * 1024);
    let (_client_reader, mut writer) = split_frames(client);
    let (mut reader, _server_writer) = split_frames(server);

    let write_both = async {
        writer
            .send_frame(&big_payload)
            .await
            .expect("in-memory duplex write cannot fail");
        writer
            .send_frame(b"tail")
            .await
            .expect("in-memory duplex write cannot fail");
    };

    let read_both = async {
        let first = reader
            .next_frame()
            .await
            .expect("in-memory duplex read cannot fail")
            .expect("the maximum-size frame was written");
        let second = reader
            .next_frame()
            .await
            .expect("in-memory duplex read cannot fail")
            .expect("the trailing frame was written");
        (first, second)
    };

    let ((), (first, second)) = tokio::join!(write_both, read_both);

    assert_eq!(first, big_frame);
    assert_eq!(second, small_frame);
}

#[tokio::test]
async fn send_frame_rejects_oversized_payload() {
    let (mut peer, writer_half) = duplex_pair(64);
    let mut writer = FrameWriter::new(writer_half);
    let payload = vec![0u8; MAX_FRAME_PAYLOAD_LEN as usize + 1];

    let error = writer
        .send_frame(&payload)
        .await
        .expect_err("payload over the limit must be rejected");

    assert!(matches!(
        error,
        ConnectionError::Framing(FramingError::PayloadTooLarge)
    ));

    drop(writer);
    let mut byte = [0u8; 1];
    let read = peer
        .read(&mut byte)
        .await
        .expect("in-memory duplex read cannot fail");
    assert_eq!(read, 0, "rejected payload must write no bytes");
}

#[tokio::test]
async fn pump_frames_forwards_frames_both_ways() {
    let (client, server) = duplex_pair(1024);
    let (inbound_tx, mut inbound_rx) = mpsc::channel(4);
    let (outbound_tx, outbound_rx) = mpsc::channel(4);

    let pump = tokio::spawn(pump_frames(client, inbound_tx, outbound_rx));
    let (mut server_reader, mut server_writer) = split_frames(server);

    server_writer
        .send_frame(b"from-peer")
        .await
        .expect("in-memory duplex write cannot fail");

    let forwarded = inbound_rx
        .recv()
        .await
        .expect("pump forwards the peer frame before closing");
    assert_eq!(forwarded, frame(b"from-peer"));

    outbound_tx
        .send(b"to-peer".to_vec())
        .await
        .expect("pump keeps the outbound channel open");

    let received = server_reader
        .next_frame()
        .await
        .expect("in-memory duplex read cannot fail")
        .expect("the pump wrote the outbound payload");
    assert_eq!(received, frame(b"to-peer"));

    drop(outbound_tx);
    let outcome = pump.await.expect("pump task does not panic");
    assert!(
        matches!(outcome, Ok(())),
        "pump returns Ok when the outbound channel closes"
    );
}
