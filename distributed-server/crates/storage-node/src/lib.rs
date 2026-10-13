//! PicBackup distributed storage-node crate.
//!
//! First structural increment: the frame header for the frame-only TCP actor
//! ([`framing`]) and a bounded, size-classed payload buffer pool
//! ([`buffer_pool`]). Networking, the Tokio actor runtime, the dedicated
//! `io_uring` disk thread, Raft, and the replication data plane arrive in later
//! increments.
//!
//! Shared v1 specification constraints later increments must keep satisfying:
//!
//! - Custom framing over TCP; the only unencrypted prefix is the protocol
//!   version and frame length, modelled by [`FrameHeader`].
//! - File payloads stream as bounded blocks with explicit buffer ownership and
//!   return; the whole file is never held in RAM.
//! - Large payload buffers are pooled across size classes with balancing;
//!   headers, control frames, and metadata use separate small buffers.
//! - Buffer shortage yields backpressure or a controlled error, never unbounded
//!   memory growth.
//! - The hot path must not synchronize through a shared `Mutex`/`RwLock`, and
//!   `tokio-uring` is not used.

pub mod buffer_pool;
pub mod framing;

pub use buffer_pool::{
    PayloadBuffer, PayloadBufferPool, PoolError, SizeClass, DEFAULT_BUFFERS_PER_CLASS,
    DEFAULT_CLASS_CAPACITIES,
};
pub use framing::{
    FrameHeader, FramingError, FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD_LEN, PROTOCOL_VERSION,
};
