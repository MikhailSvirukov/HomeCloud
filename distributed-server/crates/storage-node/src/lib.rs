//! PicBackup distributed storage-node crate.
//!
//! Structural increments so far: the frame header for the frame-only TCP actor
//! ([`framing`]), a bounded, size-classed payload buffer pool
//! ([`buffer_pool`]), the ordered payload chain and pooled chunk stream
//! ([`payload`]), the validated node identity/configuration model
//! ([`config`]), and the upload metadata model with the idempotent upload
//! ledger ([`metadata`]). Networking, the Tokio actor runtime, the dedicated
//! `io_uring` disk thread, Raft, and the replication data plane arrive in later
//! increments.
//!
//! Shared v1 specification constraints later increments must keep satisfying:
//!
//! - Custom framing over TCP; the only unencrypted prefix is the protocol
//!   version and frame length, modelled by [`FrameHeader`].
//! - File payloads stream as bounded blocks with explicit buffer ownership and
//!   return; the whole file is never held in RAM.
//! - A large payload is a logical chain of buffers including a trailing partial
//!   block; the number of buffers in flight stays bounded.
//! - Large payload buffers are pooled across size classes with balancing;
//!   headers, control frames, and metadata use separate small buffers.
//! - Buffer shortage yields backpressure or a controlled error, never unbounded
//!   memory growth.
//! - Repeated `upload_id` values are idempotent: the [`UploadLedger`] keeps the
//!   first recorded version and later retries return the stored result.
//! - The hot path must not synchronize through a shared `Mutex`/`RwLock`, and
//!   `tokio-uring` is not used.

pub mod buffer_pool;
pub mod config;
pub mod framing;
pub mod metadata;
pub mod payload;

pub use buffer_pool::{
    PayloadBuffer, PayloadBufferPool, PoolError, SizeClass, DEFAULT_BUFFERS_PER_CLASS,
    DEFAULT_CLASS_CAPACITIES,
};
pub use config::{ConfigError, NodeConfig, NodeId, MAX_NODE_ID_LEN};
pub use framing::{
    FrameHeader, FramingError, FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD_LEN, PROTOCOL_VERSION,
};
pub use metadata::{
    ContentHash, MetadataError, RecordOutcome, UploadId, UploadLedger, UploadRecord,
    CONTENT_HASH_LEN, MAX_UPLOAD_ID_LEN,
};
pub use payload::{ChunkError, ChunkStream, PayloadChain, DEFAULT_MAX_CHUNKS_IN_FLIGHT};
