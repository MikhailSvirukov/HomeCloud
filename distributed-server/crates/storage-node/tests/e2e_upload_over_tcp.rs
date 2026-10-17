//! End-to-end upload over a real loopback TCP connection.
//!
//! The client streams a payload through the public [`send_payload`] path — which
//! frames pooled chunks onto the connection writer and has to drain sealed
//! chunks under the in-flight backpressure limit — while the server reassembles
//! the payload from the whole frames yielded by the frame reader. This drives
//! the framing, payload streaming, and connection halves together over a real
//! socket instead of in-memory buffers.

use core::num::{NonZeroU32, NonZeroUsize};

use storage_node::{
    decode_frame, send_payload, split_frames, DecodeOutcome, Listener, PayloadBufferPool, SizeClass,
};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

/// Source length, chosen so the payload spans many frames and forces the
/// bounded in-flight limit to drain repeatedly.
const SOURCE_LEN: usize = 5_000;
/// Pooled chunk capacity handed to [`send_payload`].
const CHUNK_CAPACITY: usize = 256;
/// Maximum number of pooled chunks in flight, kept low to trigger the drain path.
const MAX_CHUNKS: usize = 2;

fn non_zero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test constant is non-zero")
}

fn max_frame_len() -> NonZeroU32 {
    NonZeroU32::new(64 * 1024).expect("test limit is non-zero")
}

/// Builds a payload with a non-trivial byte pattern so framing mistakes surface.
fn source_bytes() -> Vec<u8> {
    (0..SOURCE_LEN)
        .map(|index| (index * 31 % 251) as u8)
        .collect()
}

#[tokio::test]
async fn uploads_a_payload_over_tcp_and_reassembles_it() {
    let listener = Listener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");

    let (reassembled_tx, reassembled_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut reader, _writer, _peer) = listener
            .accept(max_frame_len())
            .await
            .expect("the listener accepts the client connection");

        let mut reassembled = Vec::new();
        while let Some(frame) = reader
            .next_frame()
            .await
            .expect("reading frames from the accepted connection cannot fail")
        {
            match decode_frame(&frame, max_frame_len())
                .expect("the client writes whole encoded frames")
            {
                DecodeOutcome::Complete { payload, .. } => reassembled.extend_from_slice(payload),
                DecodeOutcome::Incomplete { needed } => {
                    panic!("the frame reader yields whole frames, got incomplete need {needed}")
                }
            }
        }

        reassembled_tx
            .send(reassembled)
            .expect("the test keeps the receiver alive until the upload finishes");
    });

    let source = source_bytes();
    let mut pool = PayloadBufferPool::new(&[SizeClass::new(CHUNK_CAPACITY, 4)])
        .expect("a single increasing size class is valid");

    let client = TcpStream::connect(address)
        .await
        .expect("connecting to the bound loopback address cannot fail");
    let (client_reader, mut client_writer) = split_frames(client, max_frame_len());

    let mut source_reader = source.as_slice();
    send_payload(
        &mut client_writer,
        &mut source_reader,
        &mut pool,
        non_zero(CHUNK_CAPACITY),
        non_zero(MAX_CHUNKS),
    )
    .await
    .expect("streaming the payload over loopback cannot fail");

    drop(client_writer);
    drop(client_reader);

    let reassembled = reassembled_rx
        .await
        .expect("the server sends the reassembled payload");
    server.await.expect("the server task does not panic");

    assert_eq!(reassembled, source);
}
