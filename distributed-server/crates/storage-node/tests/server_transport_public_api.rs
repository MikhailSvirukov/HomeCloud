//! Public API integration tests for the direct encrypted inter-server transport.
//!
//! Covers `FR-CL-001 / 1`: storage nodes exchange Raft RPCs directly over TCP
//! without the dedicated server. Covers `INT-NET-021 / 3` and
//! `NFR-SEC-009 / 1`: the transport resolves a peer's Ed25519 key from the
//! trusted directory and rejects a reply signed by any other key. Covers
//! `INT-NET-024 / 2`, `INT-NET-027 / 1`, and `INT-NET-037 / 1`: every request
//! and reply is HPKE Base-mode sealed for the authenticated receiver. Covers
//! `INT-NET-072 / 2` and `INT-NET-074 / 2`: both directions carry the
//! `[3, [rpc_bytes]]` inter-server envelope. Covers `INT-NET-081 / 1`: the
//! exchange uses the signed frame with a fresh encapsulation. The reply
//! correlation uses the external-header sender and request ids.

use core::num::NonZeroU32;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use storage_node::{
    decode_raft_rpc, decode_server_message, encode_raft_rpc, encode_server_message, ClientLimits,
    ExternalHeader, FrameLength, MessageSigningKey, MessageVerifyingKey, Peer, PeerDirectory,
    PeerDirectoryError, RaftTransport, RaftTransportError, RecipientPrivateKey, RecipientPublicKey,
    ServerTransport, EXTERNAL_PROTOCOL_VERSION, FRAME_LENGTH_PREFIX_LEN, REQUEST_ID_LEN,
    SENDER_ID_LEN,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const CLIENT_SIGNING_KEY_BYTES: [u8; 32] = [7; 32];
const SERVER_SIGNING_KEY_BYTES: [u8; 32] = [8; 32];
const OTHER_SIGNING_KEY_BYTES: [u8; 32] = [6; 32];
const CLIENT_X25519_PRIVATE_KEY_BYTES: [u8; 32] = [9; 32];
const SERVER_X25519_PRIVATE_KEY_BYTES: [u8; 32] = [10; 32];
const CLIENT_SENDER_ID: [u8; SENDER_ID_LEN] = [0x11; SENDER_ID_LEN];
const SERVER_SENDER_ID: [u8; SENDER_ID_LEN] = [0x12; SENDER_ID_LEN];
const SECOND_SENDER_ID: [u8; SENDER_ID_LEN] = [0x13; SENDER_ID_LEN];
const SERVER_NODE_ID: u64 = 7;
const SECOND_NODE_ID: u64 = 8;
const MAX_FRAME_LEN: u32 = 4096;
const TEST_BIND_ADDR: &str = "127.0.0.1:0";
const UNUSED_PEER_ADDR: &str = "127.0.0.1:9";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_RPC_BYTES: &[u8] = b"raft-request-rpc-bytes";
const RESPONSE_RPC_BYTES: &[u8] = b"raft-response-rpc-bytes";
const MISMATCHED_REQUEST_ID_BYTE: u8 = 0xFF;

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

fn other_verifying_key() -> MessageVerifyingKey {
    MessageSigningKey::from_bytes(OTHER_SIGNING_KEY_BYTES).verifying_key()
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
    let non_zero = |value| std::num::NonZeroUsize::new(value).expect("the test limit is non-zero");
    ClientLimits::new(
        non_zero(1),
        non_zero(64),
        non_zero(1),
        non_zero(128),
        non_zero(64),
    )
    .expect("the test client limits are consistent")
}

fn request_envelope() -> Vec<u8> {
    encode_raft_rpc(REQUEST_RPC_BYTES).expect("the test request RPC envelope encodes")
}

fn response_envelope() -> Vec<u8> {
    encode_raft_rpc(RESPONSE_RPC_BYTES).expect("the test response RPC envelope encodes")
}

fn response_header(request_id: [u8; REQUEST_ID_LEN]) -> ExternalHeader {
    ExternalHeader {
        protocol_version: EXTERNAL_PROTOCOL_VERSION,
        sender_id: Some(SERVER_SENDER_ID),
        request_id,
        frame_number: 0,
        upload_id: None,
        responder_label: None,
        bootstrap_signing_public_key: None,
    }
}

fn peer(node_id: u64, sender_id: [u8; SENDER_ID_LEN], address: SocketAddr) -> Peer {
    let x25519_public_key = server_public_key().as_bytes();
    Peer::new(
        node_id,
        sender_id,
        address,
        &x25519_public_key,
        server_verifying_key().as_bytes(),
    )
    .expect("the fixed peer keys are valid")
}

fn peer_with_ed25519(
    node_id: u64,
    sender_id: [u8; SENDER_ID_LEN],
    address: SocketAddr,
    ed25519_public_key: [u8; 32],
) -> Peer {
    let x25519_public_key = server_public_key().as_bytes();
    Peer::new(
        node_id,
        sender_id,
        address,
        &x25519_public_key,
        ed25519_public_key,
    )
    .expect("the fixed peer keys are valid")
}

fn transport_with_peer(address: SocketAddr) -> ServerTransport {
    transport_with_directory(vec![peer(SERVER_NODE_ID, SERVER_SENDER_ID, address)])
}

fn transport_with_directory(peers: Vec<Peer>) -> ServerTransport {
    let directory = Arc::new(PeerDirectory::new(peers).expect("the test peer directory is valid"));
    ServerTransport::new(
        CLIENT_SENDER_ID,
        client_signing_key(),
        client_private_key(),
        directory,
        max_frame_len(),
        limits(),
        CONNECT_TIMEOUT,
    )
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

#[derive(Clone, Copy)]
enum ReplyRequestId {
    Echo,
    Mismatched,
}

fn mismatched_request_id(request_id: [u8; REQUEST_ID_LEN]) -> [u8; REQUEST_ID_LEN] {
    let mut mismatched = request_id;
    mismatched[0] ^= MISMATCHED_REQUEST_ID_BYTE;
    mismatched
}

async fn serve_one_connection(
    listener: TcpListener,
    expected_request: Vec<u8>,
    reply_request_id: ReplyRequestId,
) {
    let (mut stream, _) = listener
        .accept()
        .await
        .expect("the test listener accepts the transport connection");
    let frame = read_frame(&mut stream, max_frame_len())
        .await
        .expect("the test server reads the request frame");
    let message = decode_server_message(
        &frame,
        &client_verifying_key(),
        &server_private_key(),
        max_frame_len(),
        &limits(),
    )
    .expect("the test server verifies and decrypts the request");
    assert_eq!(
        message.header.sender_id,
        Some(CLIENT_SENDER_ID),
        "the request must advertise the client sender id"
    );
    assert_eq!(
        message.plaintext, expected_request,
        "the request plaintext must be the [3, [rpc_bytes]] envelope"
    );

    let request_id = match reply_request_id {
        ReplyRequestId::Echo => message.header.request_id,
        ReplyRequestId::Mismatched => mismatched_request_id(message.header.request_id),
    };
    let response = encode_server_message(
        &response_header(request_id),
        &response_envelope(),
        &client_public_key(),
        &server_signing_key(),
        max_frame_len(),
        &limits(),
    )
    .expect("the test response frame encodes");
    write_frame(&mut stream, &response, max_frame_len())
        .await
        .expect("the test server writes the response frame");
}

fn spawn_server(
    listener: TcpListener,
    expected_request: Vec<u8>,
    reply_request_id: ReplyRequestId,
) -> JoinHandle<()> {
    tokio::spawn(serve_one_connection(
        listener,
        expected_request,
        reply_request_id,
    ))
}

#[tokio::test]
async fn sends_and_receives_over_encrypted_tcp() {
    let listener = TcpListener::bind(TEST_BIND_ADDR)
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");
    let expected_request = request_envelope();
    let server = spawn_server(listener, expected_request.clone(), ReplyRequestId::Echo);
    let transport = transport_with_peer(address);

    let response = transport
        .send(SERVER_NODE_ID, expected_request)
        .await
        .expect("the test server answers the Raft RPC");

    assert_eq!(
        response,
        response_envelope(),
        "the transport returns the response [3, [rpc_bytes]] envelope"
    );
    assert_eq!(
        decode_raft_rpc(&response).expect("the response carries a Raft RPC envelope"),
        RESPONSE_RPC_BYTES,
        "the response RPC bytes survive the exchange unchanged"
    );
    server.await.expect("the test server task completes");
}

#[tokio::test]
async fn unknown_target_is_unreachable() {
    let transport = transport_with_directory(Vec::new());

    assert!(matches!(
        transport
            .send(SERVER_NODE_ID, request_envelope())
            .await,
        Err(RaftTransportError::Unreachable { target }) if target == SERVER_NODE_ID
    ));
}

#[tokio::test]
async fn rejects_a_reply_signed_by_the_wrong_server_key() {
    let listener = TcpListener::bind(TEST_BIND_ADDR)
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");
    let server = spawn_server(listener, request_envelope(), ReplyRequestId::Echo);
    let transport = transport_with_directory(vec![peer_with_ed25519(
        SERVER_NODE_ID,
        SERVER_SENDER_ID,
        address,
        other_verifying_key().as_bytes(),
    )]);

    assert!(matches!(
        transport.send(SERVER_NODE_ID, request_envelope()).await,
        Err(RaftTransportError::Send { target }) if target == SERVER_NODE_ID
    ));
    server.await.expect("the test server task completes");
}

#[tokio::test]
async fn rejects_a_mismatched_reply_request_id() {
    let listener = TcpListener::bind(TEST_BIND_ADDR)
        .await
        .expect("binding an ephemeral loopback port cannot fail");
    let address = listener
        .local_addr()
        .expect("a bound listener reports its address");
    let server = spawn_server(listener, request_envelope(), ReplyRequestId::Mismatched);
    let transport = transport_with_peer(address);

    assert!(matches!(
        transport.send(SERVER_NODE_ID, request_envelope()).await,
        Err(RaftTransportError::Send { target }) if target == SERVER_NODE_ID
    ));
    server.await.expect("the test server task completes");
}

#[tokio::test]
async fn peer_directory_rejects_duplicate_node_and_sender_ids() {
    let address: SocketAddr = UNUSED_PEER_ADDR.parse().expect("the test address parses");

    assert!(matches!(
        PeerDirectory::new(vec![
            peer(SERVER_NODE_ID, SERVER_SENDER_ID, address),
            peer(SERVER_NODE_ID, SECOND_SENDER_ID, address),
        ]),
        Err(PeerDirectoryError::DuplicateNode { node_id }) if node_id == SERVER_NODE_ID
    ));

    assert!(matches!(
        PeerDirectory::new(vec![
            peer(SERVER_NODE_ID, SERVER_SENDER_ID, address),
            peer(SECOND_NODE_ID, SERVER_SENDER_ID, address),
        ]),
        Err(PeerDirectoryError::DuplicateSender)
    ));
}
