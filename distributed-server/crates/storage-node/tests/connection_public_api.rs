//! Integration tests exercising the public `FrameConnection` API over real
//! duplex streams, so the actor codec is driven end to end rather than through
//! in-memory buffer helpers.

use storage_node::{
    encode_frame, ConnectionError, FrameConnection, FramingError, MAX_FRAME_PAYLOAD_LEN,
};
use tokio::io::{duplex, AsyncWriteExt, DuplexStream};

/// Encodes `payload`, asserting the test payload is within the frame limit.
fn frame(payload: &[u8]) -> Vec<u8> {
    encode_frame(payload).expect("test payload is within the frame limit")
}

/// Builds a connected writer/reader pair of frame connections sharing `capacity`
/// bytes of duplex buffer.
fn connected_pair(
    capacity: usize,
) -> (FrameConnection<DuplexStream>, FrameConnection<DuplexStream>) {
    let (writer_half, reader_half) = duplex(capacity);
    (
        FrameConnection::new(writer_half),
        FrameConnection::new(reader_half),
    )
}

#[tokio::test]
async fn round_trips_a_frame_over_duplex() {
    let (mut writer, mut reader) = connected_pair(1024);

    writer
        .send_frame(b"hello")
        .await
        .expect("in-memory duplex write cannot fail");

    let received = reader
        .next_frame()
        .await
        .expect("in-memory duplex read cannot fail")
        .expect("a whole frame was written");

    assert_eq!(received, frame(b"hello"));
}

#[tokio::test]
async fn next_frame_returns_none_at_eof() {
    let (writer, mut reader) = connected_pair(1024);
    drop(writer);

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

    let (mut raw_writer, reader_half) = duplex(8);
    let mut reader = FrameConnection::new(reader_half);

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
async fn send_frame_rejects_oversized_payload() {
    let (mut writer, _reader) = connected_pair(64);
    let payload = vec![0u8; MAX_FRAME_PAYLOAD_LEN as usize + 1];

    let error = writer
        .send_frame(&payload)
        .await
        .expect_err("payload over the limit must be rejected");

    assert!(matches!(
        error,
        ConnectionError::Framing(FramingError::PayloadTooLarge)
    ));
}
