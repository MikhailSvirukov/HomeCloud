//! Node runtime: serves frame connections on a bound listener.

use core::num::{NonZeroU32, NonZeroUsize};
use std::future::Future;
use std::net::SocketAddr;

use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::{debug, info, trace, warn};

use crate::connection::{pump_halves, ConnectionError, FrameReader, FrameWriter};
use crate::listener::{Listener, ListenerError};
use crate::logging::{ErrorCategory, LogEvent, LogLevel, Logger, StateChange};

type Result<T> = core::result::Result<T, ListenerError>;

/// Accepts connections until `shutdown` resolves, serving each on its own task
/// with frames bounded by `max_frame_len` and channels bounded by `queue_capacity`.
/// When a logger is supplied, accept failures and connection rejections are
/// recorded as structured events.
pub async fn serve<F>(
    listener: Listener,
    shutdown: F,
    max_frame_len: NonZeroU32,
    queue_capacity: NonZeroUsize,
    logger: Option<Logger>,
) -> Result<()>
where
    F: Future<Output = ()>,
{
    tokio::pin!(shutdown);
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept(max_frame_len) => {
                let (reader, writer, peer) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        if let Some(logger) = &logger {
                            logger.log(LogEvent::state_change(
                                LogLevel::Error,
                                StateChange::DependencyUnavailable,
                            ));
                        }
                        return Err(error);
                    }
                };
                info!(%peer, "connection accepted");
                connections.spawn(handle_connection(
                    reader,
                    writer,
                    peer,
                    queue_capacity,
                    logger.clone(),
                ));
                while connections.try_join_next().is_some() {}
            }
            () = &mut shutdown => {
                connections.abort_all();
                while connections.join_next().await.is_some() {}
                return Ok(());
            }
        }
    }
}

/// Serves one connection's frame pump until the stream or channels close.
///
/// Command handling is a later increment: received frames are drained and
/// logged, and no outbound frames are produced yet. A pump failure is reported
/// as a structured rejection when a logger is supplied.
async fn handle_connection(
    reader: FrameReader<ReadHalf<TcpStream>>,
    writer: FrameWriter<WriteHalf<TcpStream>>,
    peer: SocketAddr,
    queue_capacity: NonZeroUsize,
    logger: Option<Logger>,
) {
    let (inbound_tx, mut inbound_rx) = mpsc::channel(queue_capacity.get());
    let (outbound_tx, outbound_rx) = mpsc::channel(queue_capacity.get());

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
        Err(error) => {
            warn!(%peer, error = %error, "connection failed");
            let category = match error {
                ConnectionError::Framing(_) => ErrorCategory::InvalidRequest,
                ConnectionError::Io(_) => ErrorCategory::Internal,
            };
            if let Some(logger) = &logger {
                let peer_id = peer.to_string();
                logger.log(LogEvent::connection_rejected(Some(&peer_id), category));
            }
        }
    }
}
