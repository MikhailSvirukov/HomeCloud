//! TCP listener for the node's frame-only connections.

use std::net::SocketAddr;

use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};

use crate::connection::{split_frames, FrameReader, FrameWriter};

/// Failure modes when binding the node listener.
#[derive(Debug, thiserror::Error)]
pub enum ListenerError {
    /// The listener could not be bound or queried.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The listen address is not a valid `ip:port` socket address.
    #[error("invalid listen address {value}")]
    InvalidAddr { value: String },
}

type Result<T> = core::result::Result<T, ListenerError>;

/// Bound TCP listener that yields a frame reader/writer per connection.
pub struct Listener {
    listener: TcpListener,
}

impl Listener {
    /// Binds `listen_addr`, an `ip:port` string.
    pub async fn bind(listen_addr: &str) -> Result<Self> {
        let address =
            listen_addr
                .parse::<SocketAddr>()
                .map_err(|_| ListenerError::InvalidAddr {
                    value: listen_addr.to_owned(),
                })?;
        let listener = TcpListener::bind(address).await?;
        Ok(Self { listener })
    }

    /// The address the listener is bound to.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        let address = self.listener.local_addr()?;
        Ok(address)
    }

    /// Accepts one connection, returning its frame reader/writer halves and the
    /// peer address.
    pub async fn accept(
        &self,
    ) -> Result<(
        FrameReader<ReadHalf<TcpStream>>,
        FrameWriter<WriteHalf<TcpStream>>,
        SocketAddr,
    )> {
        let (stream, peer) = self.listener.accept().await?;
        let (reader, writer) = split_frames(stream);
        Ok((reader, writer, peer))
    }
}
