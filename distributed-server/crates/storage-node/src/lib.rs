//! PicBackup distributed storage-node crate.
//!
//! Structural increments so far: the frame header for the frame-only TCP actor,
//! whole-frame encode/decode over byte slices, a bounded, size-classed payload
//! buffer pool, the ordered payload chain and pooled chunk stream, the
//! validated node identity/configuration model, the upload metadata model with
//! the idempotent upload ledger, the receive-side frame buffer, the typed
//! upload outcome codes ([`UploadStatus`] and [`ResponseError`]), and the
//! frame-only connection ([`FrameConnection`]) that reads and writes whole
//! frames over an async byte stream.
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

mod buffer_pool;
mod codec;
mod config;
mod connection;
mod frame_buffer;
mod framing;
mod metadata;
mod payload;
mod response;

pub use buffer_pool::{
    PayloadBuffer, PayloadBufferPool, PoolError, SizeClass, DEFAULT_BUFFERS_PER_CLASS,
    DEFAULT_CLASS_CAPACITIES,
};
pub use codec::{decode_frame, encode_frame, DecodeOutcome};
pub use config::{ConfigError, NodeConfig, NodeId, MAX_NODE_ID_LEN};
pub use connection::{ConnectionError, FrameConnection};
pub use frame_buffer::{FrameBuffer, MAX_BUFFERED_FRAME_LEN};
pub use framing::{
    FrameHeader, FramingError, FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD_LEN, PROTOCOL_VERSION,
};
pub use metadata::{
    ContentHash, MetadataError, RecordOutcome, UploadId, UploadLedger, UploadRecord,
    CONTENT_HASH_LEN, MAX_UPLOAD_ID_LEN,
};
pub use payload::{ChunkError, ChunkStream, PayloadChain, DEFAULT_MAX_CHUNKS_IN_FLIGHT};
pub use response::{ResponseError, UploadStatus};
