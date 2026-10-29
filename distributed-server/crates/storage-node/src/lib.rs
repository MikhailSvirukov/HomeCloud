//! PicBackup distributed storage-node crate.
//!
//! Structural increments so far: the 4-byte big-endian frame-length prefix for
//! the frame-only TCP actor, whole-frame encode/decode over byte slices, a
//! bounded, size-classed payload buffer pool, the ordered payload chain and
//! pooled chunk stream, the validated node identity/configuration model, the
//! upload metadata model with the idempotent upload ledger, the receive-side
//! frame buffer with inbound sequence validation primitives ([`FrameSequence`],
//! [`FrameSequenceError`]), protocol version-list negotiation primitives
//! ([`encode_version_list`], [`decode_version_list`], [`negotiate_version`],
//! [`VersionError`], [`SUPPORTED_VERSION`], [`VERSION_LIST`]), the typed upload
//! outcome codes ([`UploadStatus`] and
//! [`ResponseError`]), and the frame reader/writer halves ([`FrameReader`],
//! [`FrameWriter`]) with [`split_frames`] and the forwarding [`pump_frames`]
//! plus its split-half variant [`pump_halves`], which move whole frames over an
//! async byte stream, the payload streaming path ([`send_payload`],
//! [`write_chain`]) that frames pooled chunks onto a [`FrameWriter`], and the
//! TCP listener ([`Listener`]) that yields frame reader/writer halves per
//! accepted connection, and the node runtime ([`serve`]) that accepts
//! connections until shutdown and serves each frame pump on its own task, and
//! the bounded CPU pool ([`CpuPool`]) that runs CPU-bound work off the Tokio
//! worker threads, the on-disk storage layout ([`StorageLayout`]) with
//! separate accounts and Raft roots and hex-encoded account/blob paths, and
//! private-key file loading ([`PrivateKey`], [`KeyError`]) from configured
//! owner-only files, Ed25519 message signing/verification
//! ([`MessageSigningKey`], [`MessageVerifyingKey`], [`SigningError`]), and HPKE
//! Base-mode message sealing ([`seal`], [`open`]) with X25519, HKDF-SHA-256, and
//! AES-256-GCM, and the local
//! OpenRaft runtime ([`RaftNode`]) that restores persisted Raft state from disk,
//! commits the full upload version metadata, including the SHA-256, the
//! client-supplied capture/creation time, and the server acceptance time
//! (`FR-NODE-008 / 1`), together with the blob replica set to a catalog state
//! machine, exchanges CBOR-encoded RPCs inside the type-3 inter-server envelope
//! over [`RaftTransport`], and keeps a durable bootstrap marker. The
//! inter-server Raft RPC envelope ([`encode_raft_rpc`], [`decode_raft_rpc`])
//! and its message-type registry ([`MessageType`], [`MessageError`]) are also
//! part of the public wire API, together with the catalog entry, page, and
//! request CBOR codecs ([`CatalogEntry`], [`CatalogPage`], [`CatalogRequest`],
//! [`encode_page`], [`decode_request`]), and the catalog client exchange codecs
//! ([`ExternalHeader`], [`CatalogPageRequest`], [`encode_external_header`],
//! [`decode_external_header`], [`encode_catalog_page_request`],
//! [`decode_catalog_page_request`], [`encode_catalog_page_response`]) that
//! assemble the seven-position external header, the client logical header, and
//! the result envelope, the upload request header codec
//! ([`UploadRequest`], [`UploadRequestHeader`], [`decode_upload_request`],
//! [`encode_upload_request`]) that validates the upload operation arguments, and
//! CSPRNG-backed catalog pagination
//! snapshots ([`CatalogPager`], [`CatalogPagerError`]), and the catalog service
//! ([`CatalogService`], [`CatalogServiceError`]) that maps committed versions to
//! entries and serves paginated catalog pages, and the catalog protocol error
//! codes ([`CatalogErrorCode`]) for invalid cursors, expired snapshots, and
//! internal failures, and the catalog page handler ([`CatalogPageHandler`],
//! [`CatalogHandlerError`]) that chooses the first or continuation page and
//! encodes the `[result_code, result_args_array]` response, and local blob
//! integrity verification ([`verify_local_blob`],
//! [`BlobIntegrity`], [`IntegrityError`]) that streams a committed blob, compares
//! its SHA-256 with the committed hash, and marks a mismatch with a
//! `<blob>.corrupt` sidecar marker, and the periodic integrity scanner
//! ([`scan_committed_blobs`], [`run_integrity_scan_loop`]) that verifies all
//! committed blobs on a configured schedule with a bounded concurrency limit.
//! Direct durable blob writes ([`write_blob_direct`], [`StoredBlob`],
//! [`BlobWriteError`]) stream a verified payload into its final location and
//! fsync the file and its parent directory before reporting it durable.
//! Upload failures map to the published protocol result codes in
//! [`UploadErrorCode`], covering request, authentication, file, frame-sequence,
//! upload-id, capacity, and internal error classes (`INT-NET-064 / 1`).
//! The Prometheus-compatible metrics endpoint
//! ([`MetricsServer`]) serves `GET /metrics` on a separately configured
//! monitoring listener, using plaintext HTTP only on loopback and TLS with the
//! configured [`MetricsTlsConfig`] certificate and private key elsewhere.
//! Structured UTF-8 JSON Lines logging
//! ([`LogEvent`], [`Logger`]) provides a bounded, non-blocking event API with
//! size- and age-based rotation on the configured path.
//!
//! Shared v1 specification constraints later increments must keep satisfying:
//!
//! - Custom framing over TCP; the only unencrypted prefix is a 4-byte
//!   big-endian payload length, modelled by [`FrameLength`]. The length counts
//!   the bytes after the prefix and excludes the prefix itself, and receivers
//!   validate it against the configured maximum before buffering the frame.
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

mod blob_writer;
mod buffer_pool;
mod catalog;
mod catalog_error;
mod catalog_exchange;
mod catalog_handler;
mod catalog_pager;
mod catalog_service;
mod codec;
mod config;
mod connection;
mod cpu_pool;
mod frame_buffer;
mod frame_sequence;
mod framing;
mod integrity;
mod integrity_scan;
mod keys;
mod listener;
mod logging;
mod message;
mod metadata;
mod metrics;
mod node;
mod payload;
mod payload_stream;
mod raft;
mod response;
mod sealing;
mod signing;
mod storage_layout;
mod upload_error;
mod upload_exchange;
mod version_negotiation;

pub use blob_writer::{write_blob_direct, BlobWriteError, StoredBlob};
pub use buffer_pool::{
    PayloadBuffer, PayloadBufferPool, PoolError, SizeClass, DEFAULT_BUFFERS_PER_CLASS,
    DEFAULT_CLASS_CAPACITIES,
};
pub use catalog::{
    decode_request, encode_page, CatalogEntry, CatalogError, CatalogPage, CatalogRequest,
    CATALOG_TOKEN_LEN,
};
pub use catalog_error::CatalogErrorCode;
pub use catalog_exchange::{
    decode_catalog_page_request, decode_external_header, encode_catalog_page_request,
    encode_catalog_page_response, encode_external_header, CatalogExchangeError, CatalogPageRequest,
    ExternalHeader, BOOTSTRAP_KEY_LEN, EXTERNAL_PROTOCOL_VERSION, REQUEST_ID_LEN, SENDER_ID_LEN,
};
pub use catalog_handler::{CatalogHandlerError, CatalogPageHandler};
pub use catalog_pager::{CatalogPager, CatalogPagerError};
pub use catalog_service::{CatalogService, CatalogServiceError};
pub use codec::{decode_frame, encode_frame, DecodeOutcome};
pub use config::{ConfigError, NodeConfig, NodeId, MAX_NODE_ID_LEN};
pub use connection::{
    pump_frames, pump_halves, split_frames, ConnectionError, FrameReader, FrameWriter,
};
pub use cpu_pool::{CpuPool, CpuPoolError};
pub use frame_buffer::FrameBuffer;
pub use frame_sequence::{FrameSequence, FrameSequenceError};
pub use framing::{FrameLength, FramingError, FRAME_LENGTH_PREFIX_LEN, PROTOCOL_VERSION};
pub use integrity::{verify_local_blob, BlobIntegrity, IntegrityError, CORRUPT_MARKER_SUFFIX};
pub use integrity_scan::{
    run_integrity_scan_loop, scan_committed_blobs, IntegrityScanConfig, IntegrityScanReport,
};
pub use keys::{KeyError, PrivateKey, KEY_LEN};
pub use listener::{Listener, ListenerError};
pub use logging::{
    start_logging, ErrorCategory, EventType, LogConfig, LogConfigError, LogEvent, LogLevel, Logger,
    Logging, OperationType, Outcome, StateChange,
};
pub use message::{decode_raft_rpc, encode_raft_rpc, MessageError, MessageType};
pub use metadata::{
    ContentHash, MetadataError, RecordOutcome, UploadId, UploadLedger, UploadRecord,
    CONTENT_HASH_LEN, UPLOAD_ID_LEN,
};
pub use metrics::{Metrics, MetricsServer, MetricsTlsConfig};
pub use node::serve;
pub use payload::{ChunkError, ChunkStream, PayloadChain, DEFAULT_MAX_CHUNKS_IN_FLIGHT};
pub use payload_stream::{send_payload, write_chain, StreamError};
pub use raft::{
    CommittedVersion, RaftError, RaftNode, RaftRequest, RaftResponse, RaftTiming, RaftTransport,
    RaftTransportError,
};
pub use response::{ResponseError, UploadStatus};
pub use sealing::{
    open, seal, RecipientPrivateKey, RecipientPublicKey, SealingError, ENCAPSULATED_KEY_LEN,
    X25519_KEY_LEN,
};
pub use signing::{
    MessageSigningKey, MessageVerifyingKey, SigningError, SIGNATURE_LEN, SIGNING_KEY_LEN,
};
pub use storage_layout::{LayoutError, StorageLayout};
pub use upload_error::UploadErrorCode;
pub use upload_exchange::{
    decode_upload_request, encode_upload_request, UploadExchangeError, UploadRequest,
    UploadRequestHeader,
};
pub use version_negotiation::{
    decode_version_list, encode_version_list, negotiate_version, VersionError, SUPPORTED_VERSION,
    VERSION_LIST,
};
