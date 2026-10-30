//! Public API integration tests for the incoming encrypted peer session.
//!
//! Covers `FR-CL-001 / 1`: storage nodes exchange Raft RPCs directly over TCP
//! without the dedicated server. Covers `FR-CL-031 / 1` and `INT-NET-074 / 2`:
//! the session hands the decrypted `[3, [rpc_bytes]]` plaintext to the local
//! handler and returns its reply. Covers `INT-NET-021 / 3` and `NFR-SEC-009 / 1`:
//! the session resolves the sender in the trusted directory before verifying and
//! decrypting, and rejects an unknown sender without dispatch. Covers
//! `INT-NET-024 / 2`, `INT-NET-027 / 1`, and `INT-NET-037 / 1`: the request and
//! reply are HPKE Base-mode sealed for the authenticated receiver. Covers
//! `INT-NET-072 / 2`: the plaintext is the `[message_type, args]` envelope.
//! Covers `INT-NET-081 / 1`: the exchange uses signed application frames with a
//! fresh encapsulation. The reply repeats the request id, uses frame number
//! zero, carries no upload or bootstrap field, and names the local responder.

use core::num::{NonZeroU32, NonZeroUsize};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use storage_node::{
    decode_raft_rpc, decode_server_message, encode_raft_rpc, encode_server_message,
    serve_peer_connection, ClientLimits, ExternalHeader, FrameLength, MessageSigningKey,
    MessageVerifyingKey, Peer, PeerDirectory, RaftError, RaftTransport, RaftTransportError,
    RecipientPrivateKey, RecipientPublicKey, ServerSessionError, ServerTransport,
    EXTERNAL_PROTOCOL_VERSION, FRAME_LENGTH_PREFIX_LEN, REQUEST_ID_LEN, SENDER_ID_LEN,
    UPLOAD_ID_LEN,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const CLIENT_SIGNING_KEY_BYTES: [u8; 32] = [7; 32];
const SERVER_SIGNING_KEY_BYTES: [u8; 32] = [8; 32];
const CLIENT_X25519_PRIVATE_KEY_BYTES: [u8; 32] = [9; 32];
const SERVER_X25519_PRIVATE_KEY_BYTES: [u8; 32] = [10; 32];
const CLIENT_SENDER_ID: [u8; SENDER_ID_LEN] = [0x11; SENDER_ID_LEN];
const SERVER_SENDER_ID: [u8; SENDER_ID_LEN] = [0x12; SENDER_ID_LEN];
const CLIENT_NODE_ID: u64 = 6;
const SERVER_NODE_ID: u64 = 7;
const SERVER_LABEL: &str = "storage-1";
const REQUEST_ID: [u8; REQUEST_ID_LEN] = [0x22; REQUEST_ID_LEN];
const MAX_FRAME_LEN: u32 = 4096;
const TEST_BIND_ADDR: &str = "127.0.0.1:0";
const UNUSED_PEER_ADDR: &str = "127.0.0.1:9";
const RPC_BYTES: &[u8] = b"raft-request-rpc-bytes";
const UPLOAD_ID_BYTES: [u8; UPLOAD_ID_LEN] = [0xAA; UPLOAD_ID_LEN];

fn client_signing_key() -> MessageSigningKey {
    MessageSigningKey::from_bytes(CLIENT_SIGNING_KEY_BYTES)
}

fn client_verifying_key() -> MessageVerifyingKey {
    client_signing_key().verifying_key()
}

fn server_signing_key() -> MessageSigningKey {
    MessageSigningKey::from_bytes(SERVER_SIGNING_KEY_BYTES)
}

fn server_verifying_key() -> MessageVerifyingKey {
    server_signing_key().verifying_key()
}

fn client_private_key() -> RecipientPrivateKey {
    RecipientPrivateKey::from_bytes(&CLIENT_X25519_PRIVATE_KEY_BYTES)
        .expect("the fixed client X25519 private key is valid")
}

fn client_public_key() -> RecipientPublicKey {
    client_private_key().public_key()
}

fn server_private_key() -> RecipientPrivateKey {
    RecipientPrivateKey::from_bytes(&SERVER_X25519_PRIVATE_KEY_BYTES)
        .expect("the fixed server X25519 private key is valid")
}

fn server_public_key() -> RecipientPublicKey {
    server_private_key().public_key()
}

fn max_frame_len() -> NonZeroU32 {
    NonZeroU32::new(MAX_FRAME_LEN).expect("the test frame limit is non-zero")
}

fn limits() -> ClientLimits {
    let non_zero = |value| NonZeroUsize::new(value).expect("the test limit is non-zero");
    ClientLimits::new(
        non_zero(1),
        non_zero(64),
        non_zero(1),
        non_zero(128),
        non_zero(64),
    )
    .expect("the test client limits are consistent")
}

fn rpc_envelope() -> Vec<u8> {
    encode_raft_rpc(RPC_BYTES).expect("the test Raft RPC envelope encodes")
}

fn ordinary_header(request_id: [u8; REQUEST_ID_LEN]) -> ExternalHeader {
    ExternalHeader {
        protocol_version: EXTERNAL_PROTOCOL_VERSION,
        sender_id: Some(CLIENT_SENDER_ID),
        request_id,
        frame_number: 0,
        upload_id: None,
        responder_label: None,
        bootstrap_signing_public_key: None,
    }
}

fn peer_with_keys(
    node_id: u64,
    sender_id: [u8; SENDER_ID_LEN],
    address: SocketAddr,
    x25519_public_key: &RecipientPublicKey,
    ed25519_public_key: &MessageVerifyingKey,
) -> Peer {
    Peer::new(
        node_id,
        sender_id,
        address,
        &x25519_public_key.as_bytes(),
        ed25519_public_key.as_bytes(),
    )
    .expect("the fixed peer keys are valid")
}

fn client_peer() -> Peer {
    let address = UNUSED_PEER_ADDR
        .parse()
        .expect("the placeholder peer address parses");
    peer_with_keys(
        CLIENT_NODE_ID,
        CLIENT_SENDER_ID,
        address,
        &client_public_key(),
        &client_verifying_key(),
    )
}

fn server_directory() -> Arc<PeerDirectory> {
    Arc::new(
        PeerDirectory::new(vec![client_peer()])
            .expect("the test server peer directory contains one client"),
    )
}

fn empty_directory() -> Arc<PeerDirectory> {
    Arc::new(PeerDirectory::new(Vec::new()).expect("an empty peer directory is valid"))
}

fn client_transport(address: SocketAddr) -> ServerTransport {
    let server = peer_with_keys(
        SERVER_NODE_ID,
        SERVER_SENDER_ID,
        address,
        &server_public_key(),
        &server_verifying_key(),
    );
    let peers = Arc::new(
        PeerDirectory::new(vec![server]).expect("the test client peer directory is valid"),
    );
    ServerTransport::new(
        CLIENT_SENDER_ID,
        client_signing_key(),
        client_private_key(),
        peers,
        max_frame_len(),
        limits(),
        std::time::Duration::from_secs(2),
    )
}

async fn bind_loopback() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind(TEST_BIND_ADDR)
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");
    (listener, address)
}

async fn read_frame<R>(reader: &mut R, max_frame_len: NonZeroU32) -> io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0u8; FRAME_LENGTH_PREFIX_LEN];
    reader.read_exact(&mut prefix).await?;
    let length = FrameLength::decode(prefix, max_frame_len).map_err(io::Error::other)?;
    let mut frame = vec![0u8; FRAME_LENGTH_PREFIX_LEN + length.get() as usize];
    frame[..FRAME_LENGTH_PREFIX_LEN].copy_from_slice(&prefix);
    reader
        .read_exact(&mut frame[FRAME_LENGTH_PREFIX_LEN..])
        .await?;
    Ok(frame)
}

async fn write_frame<W>(writer: &mut W, frame: &[u8], max_frame_len: NonZeroU32) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let prefix: [u8; FRAME_LENGTH_PREFIX_LEN] = frame
        .get(..FRAME_LENGTH_PREFIX_LEN)
        .and_then(|prefix| prefix.try_into().ok())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "the frame has no length prefix")
        })?;
    let length = FrameLength::decode(prefix, max_frame_len).map_err(io::Error::other)?;
    if frame.len() != FRAME_LENGTH_PREFIX_LEN + length.get() as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the frame does not match its length prefix",
        ));
    }
    writer.write_all(frame).await
}

fn spawn_session<H, F>(
    listener: TcpListener,
    peers: Arc<PeerDirectory>,
    handler: H,
) -> JoinHandle<Result<(), ServerSessionError>>
where
    H: FnOnce(Vec<u8>) -> F + Send + 'static,
    F: Future<Output = Result<Vec<u8>, RaftError>> + Send + 'static,
{
    tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("the session listener accepts one peer connection");
        let signing_key = server_signing_key();
        let recipient_key = server_private_key();
        serve_peer_connection(
            stream,
            SERVER_SENDER_ID,
            SERVER_LABEL,
            &signing_key,
            &recipient_key,
            &peers,
            max_frame_len(),
            &limits(),
            handler,
        )
        .await
    })
}

#[tokio::test]
async fn dispatches_an_encrypted_rpc_and_returns_a_correlated_reply() {
    let (listener, address) = bind_loopback().await;
    let server = spawn_session(listener, server_directory(), |plaintext| async move {
        Ok(plaintext)
    });
    let transport = client_transport(address);
    let request = rpc_envelope();

    let response = transport
        .send(SERVER_NODE_ID, request.clone())
        .await
        .expect("the peer session answers the encrypted Raft RPC");

    assert_eq!(
        response, request,
        "the reply envelope must equal the echoed request envelope"
    );
    assert_eq!(
        decode_raft_rpc(&response).expect("the reply carries a Raft RPC envelope"),
        RPC_BYTES,
        "the RPC bytes must survive the encrypted round trip unchanged"
    );
    let served = server.await.expect("the peer session task completes");
    assert!(matches!(served, Ok(())));
}

#[tokio::test]
async fn rejects_an_unknown_sender_without_dispatch() {
    let (listener, address) = bind_loopback().await;
    let dispatched = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&dispatched);
    let server = spawn_session(listener, empty_directory(), move |_plaintext| {
        handler_flag.store(true, Ordering::SeqCst);
        async { Ok(Vec::new()) }
    });
    let transport = client_transport(address);

    let sent = transport.send(SERVER_NODE_ID, rpc_envelope()).await;

    assert!(matches!(
        sent,
        Err(RaftTransportError::Send { target }) if target == SERVER_NODE_ID
    ));
    let served = server.await.expect("the peer session task completes");
    assert!(matches!(served, Err(ServerSessionError::UnknownSender)));
    assert!(
        !dispatched.load(Ordering::SeqCst),
        "an unknown sender must not reach the local handler"
    );
}

#[tokio::test]
async fn handler_error_produces_no_application_reply() {
    let (listener, address) = bind_loopback().await;
    let server = spawn_session(listener, server_directory(), |_plaintext| async {
        Err(RaftError::Decode)
    });
    let transport = client_transport(address);

    let sent = transport.send(SERVER_NODE_ID, rpc_envelope()).await;

    assert!(matches!(
        sent,
        Err(RaftTransportError::Send { target }) if target == SERVER_NODE_ID
    ));
    let served = server.await.expect("the peer session task completes");
    assert!(matches!(served, Err(ServerSessionError::Handler(..))));
}

#[tokio::test]
async fn response_carries_the_local_responder_label() {
    let (listener, address) = bind_loopback().await;
    let server = spawn_session(listener, server_directory(), |plaintext| async move {
        Ok(plaintext)
    });
    let request = encode_server_message(
        &ordinary_header(REQUEST_ID),
        &rpc_envelope(),
        &server_public_key(),
        &client_signing_key(),
        max_frame_len(),
        &limits(),
    )
    .expect("the raw client request frame encodes");

    let mut stream = TcpStream::connect(address)
        .await
        .expect("the raw client connects to the peer session");
    write_frame(&mut stream, &request, max_frame_len())
        .await
        .expect("the raw client writes its request frame");
    let reply_frame = read_frame(&mut stream, max_frame_len())
        .await
        .expect("the peer session writes a response frame");
    let reply = decode_server_message(
        &reply_frame,
        &server_verifying_key(),
        &client_private_key(),
        max_frame_len(),
        &limits(),
    )
    .expect("the client verifies and decrypts the response");

    assert_eq!(
        reply.plaintext,
        rpc_envelope(),
        "the response plaintext must be the echoed request envelope"
    );
    assert_eq!(
        reply.header.request_id, REQUEST_ID,
        "the response must carry the request id"
    );
    assert_eq!(
        reply.header.frame_number, 0,
        "the response must use frame number zero"
    );
    assert!(
        reply.header.upload_id.is_none(),
        "an RPC response must not carry an upload id"
    );
    assert!(
        reply.header.bootstrap_signing_public_key.is_none(),
        "an RPC response must not carry a bootstrap key"
    );
    assert_eq!(
        reply.header.responder_label.as_deref(),
        Some(SERVER_LABEL),
        "the response must name the local responder"
    );
    let served = server.await.expect("the peer session task completes");
    assert!(matches!(served, Ok(())));
}

#[tokio::test]
async fn rejects_a_nonordinary_request_header() {
    let (listener, address) = bind_loopback().await;
    let dispatched = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&dispatched);
    let server = spawn_session(listener, server_directory(), move |_plaintext| {
        handler_flag.store(true, Ordering::SeqCst);
        async { Ok(Vec::new()) }
    });
    let mut header = ordinary_header(REQUEST_ID);
    header.upload_id = Some(UPLOAD_ID_BYTES);
    let request = encode_server_message(
        &header,
        &rpc_envelope(),
        &server_public_key(),
        &client_signing_key(),
        max_frame_len(),
        &limits(),
    )
    .expect("the nonordinary client request frame encodes");

    let mut stream = TcpStream::connect(address)
        .await
        .expect("the raw client connects to the peer session");
    write_frame(&mut stream, &request, max_frame_len())
        .await
        .expect("the raw client writes its request frame");
    let served = server.await.expect("the peer session task completes");

    assert!(matches!(served, Err(ServerSessionError::InvalidHeader)));
    assert!(
        !dispatched.load(Ordering::SeqCst),
        "a nonordinary request header must not reach the local handler"
    );
    let mut probe = [0u8; 1];
    let read = stream.read(&mut probe).await;
    assert!(
        matches!(read, Ok(0)),
        "the rejected session must not write an application reply"
    );
}
