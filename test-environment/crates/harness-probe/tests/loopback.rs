//! Loopback tests for the bounded raw TCP wire probe (`TST-021/1`).
//!
//! Each test binds a real loopback listener, drives one probe scenario against
//! a server thread, and joins that thread so server panics fail the test.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use harness_probe::{ProbeError, WireProbe};
use harness_wire::frame::{self, Frame, FrameError};

/// `CFG-CL-011` frame body limit used by the probes.
const MAX_FRAME_LEN: u32 = 1024;
/// Connect, read, and write timeout used by the probes.
const TIMEOUT: Duration = Duration::from_secs(1);

/// The contract's minimal valid frame header (`[1]`).
const CONTRACT_HEADER: [u8; 2] = [0x81, 0x01];
/// Signature bytes of the contract frame.
const SIGNATURE: [u8; 64] = [0xAA; 64];
/// Payload bytes of the contract frame.
const PAYLOAD: [u8; 2] = [0xDE, 0xAD];

/// The contract frame the probe sends and reads.
fn contract_frame() -> Frame {
    Frame {
        header: CONTRACT_HEADER.to_vec(),
        signature: SIGNATURE,
        payload: PAYLOAD.to_vec(),
    }
}

/// Binds a loopback listener and runs `handler` for one accepted connection.
fn serve_once<F>(handler: F) -> (SocketAddr, JoinHandle<()>)
where
    F: FnOnce(TcpStream) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener must bind");
    let address = listener
        .local_addr()
        .expect("bound listener must have an address");
    let server = thread::spawn(move || {
        let (stream, _) = listener
            .accept()
            .expect("server must accept one connection");
        handler(stream);
    });
    (address, server)
}

/// Joins the server thread, propagating any panic.
fn join(server: JoinHandle<()>) {
    server.join().expect("server thread must not panic");
}

/// `TST-021/1`: the probe reads one complete length-prefixed frame.
#[test]
fn reads_a_framed_response() {
    let frame = contract_frame();
    let bytes = frame::encode(&frame, MAX_FRAME_LEN).expect("contract frame is valid");
    let expected = frame.clone();
    let (address, server) = serve_once(move |mut stream| {
        stream
            .write_all(&bytes)
            .expect("server must write the framed response");
    });

    let mut probe = WireProbe::connect(address, TIMEOUT).expect("probe must connect");

    assert_eq!(
        probe
            .read_frame(MAX_FRAME_LEN)
            .expect("probe must read the frame"),
        expected
    );
    join(server);
}

/// `INT-NET-004/1`, `TST-021/1`: a prefix above the configured limit is
/// rejected before the body is read.
#[test]
fn rejects_a_declared_length_above_the_limit() {
    let (address, server) = serve_once(|mut stream| {
        stream
            .write_all(&[0x00, 0x00, 0x10, 0x00])
            .expect("server must write the oversized prefix");
        thread::sleep(Duration::from_millis(200));
    });

    let mut probe = WireProbe::connect(address, TIMEOUT).expect("probe must connect");

    let outcome = probe.read_frame(MAX_FRAME_LEN);
    match outcome {
        Err(ProbeError::InvalidFrame(FrameError::BodyTooLarge { declared, limit })) => {
            assert_eq!(declared, 4096);
            assert_eq!(limit, MAX_FRAME_LEN);
        }
        other => panic!("expected a body-too-large rejection, got {other:?}"),
    }
    join(server);
}

/// `TST-021/1`: `send_frame` writes exactly `frame::encode`'s bytes.
#[test]
fn send_frame_writes_the_encoded_body() {
    let frame = contract_frame();
    let expected = frame::encode(&frame, MAX_FRAME_LEN).expect("contract frame is valid");
    let declared = u32::from_be_bytes(
        expected[..4]
            .try_into()
            .expect("frame begins with a four-byte prefix"),
    );
    let declared_len = usize::try_from(declared).expect("declared length fits usize");
    let (sender, receiver) = mpsc::channel();
    let (address, server) = serve_once(move |mut stream| {
        let mut buffer = vec![0u8; 4 + declared_len];
        stream
            .read_exact(&mut buffer)
            .expect("server must read exactly one frame");
        sender.send(buffer).expect("test must receive the bytes");
    });

    let mut probe = WireProbe::connect(address, TIMEOUT).expect("probe must connect");
    probe
        .send_frame(&frame, MAX_FRAME_LEN)
        .expect("probe must send the frame");

    let received = receiver
        .recv_timeout(TIMEOUT)
        .expect("server must report the received bytes");
    assert_eq!(received, expected);
    join(server);
}

/// `INT-NET-068/1`, `TST-021/1`: a peer that closes without a response is the
/// expected outcome of a rejected request.
#[test]
fn expect_closed_succeeds_when_the_peer_closes() {
    let (address, server) = serve_once(drop);

    let mut probe = WireProbe::connect(address, TIMEOUT).expect("probe must connect");

    assert!(
        matches!(probe.expect_closed(Duration::from_millis(500)), Ok(())),
        "the probe must observe the close without a response"
    );
    join(server);
}

/// `TST-021/1`: a peer that stays open is reported as a timeout.
#[test]
fn expect_closed_reports_an_open_peer() {
    let (address, server) = serve_once(|stream| {
        thread::sleep(Duration::from_millis(800));
        drop(stream);
    });

    let mut probe = WireProbe::connect(address, TIMEOUT).expect("probe must connect");

    let outcome = probe.expect_closed(Duration::from_millis(200));
    match outcome {
        Err(ProbeError::Timeout(timeout)) => assert_eq!(timeout, Duration::from_millis(200)),
        other => panic!("expected a timeout, got {other:?}"),
    }
    join(server);
}

/// `TST-021/1`: any byte instead of a close is reported as unexpected data.
#[test]
fn expect_closed_reports_unexpected_data() {
    let (address, server) = serve_once(|mut stream| {
        stream
            .write_all(&[0x42])
            .expect("server must write the byte");
        thread::sleep(Duration::from_millis(300));
    });

    let mut probe = WireProbe::connect(address, TIMEOUT).expect("probe must connect");

    let outcome = probe.expect_closed(Duration::from_millis(500));
    match outcome {
        Err(ProbeError::UnexpectedData(0x42)) => {}
        other => panic!("expected unexpected data, got {other:?}"),
    }
    join(server);
}
