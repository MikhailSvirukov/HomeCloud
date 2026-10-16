//! Node runtime: binds the listen address and serves frame connections.

use std::future::Future;
use std::net::SocketAddr;

use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, info, trace, warn};

use crate::connection::{pump_halves, FrameReader, FrameWriter};

/// Frames buffered per direction for one connection.
const CONNECTION_CHANNEL_CAPACITY: usize = 16;

/// Accepts connections until `shutdown` resolves, serving each on its own task.
pub async fn serve<F>(
    listener: crate::listener::Listener,
    shutdown: F,
) -> Result<(), crate::listener::ListenerError>
where
    F: Future<Output = ()>,
{
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (reader, writer, peer) = accepted?;
                info!(%peer, "connection accepted");
                tokio::spawn(handle_connection(reader, writer, peer));
            }
            () = &mut shutdown => return Ok(()),
        }
    }
}

/// Serves one connection's frame pump until the stream or channels close.
///
/// Command handling is a later increment: received frames are drained and
/// logged, and no outbound frames are produced yet.
async fn handle_connection(
    reader: FrameReader<ReadHalf<TcpStream>>,
    writer: FrameWriter<WriteHalf<TcpStream>>,
    peer: SocketAddr,
) {
    let (inbound_tx, mut inbound_rx) = mpsc::channel(CONNECTION_CHANNEL_CAPACITY);
    let (outbound_tx, outbound_rx) = mpsc::channel(CONNECTION_CHANNEL_CAPACITY);

    let _outbound = outbound_tx;

    let (pump, ()) = tokio::join!(
        pump_halves(reader, writer, inbound_tx, outbound_rx),
        async {
            while let Some(frame) = inbound_rx.recv().await {
                debug!(%peer, frame_len = frame.len(), "frame received");
            }
        },
    );

    match pump {
        Ok(()) => trace!(%peer, "connection closed"),
        Err(error) => warn!(%peer, error = %error, "connection failed"),
    }
}
