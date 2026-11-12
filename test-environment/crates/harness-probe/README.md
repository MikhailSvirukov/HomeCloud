# harness-probe

Bounded raw TCP wire probe for negative protocol scenarios (`TST-021 / 1`). It
sends raw bytes, reads one length-prefixed frame with a caller-supplied body
limit, and asserts that a peer closes the connection without an application
response. It performs no cryptography and no interpretation of the payload.

## API

- `WireProbe::connect(address, timeout)` — bounded connect plus read and write
  timeouts for every later operation.
- `WireProbe::send(bytes)` / `send_frame(&Frame, max_frame_len)` — write exactly
  the given bytes or the encoded frame.
- `WireProbe::read_frame(max_frame_len)` — read the 4-byte big-endian prefix and
  exactly the declared body; a prefix above `max_frame_len` is rejected before
  the body is read (`CFG-CL-011`, `INT-NET-004 / 1`).
- `WireProbe::expect_closed(timeout)` — require a close without a response
  (`INT-NET-068 / 1`).

## Errors

- `ProbeError::Timeout` — the peer neither answered nor closed within the bound.
- `ProbeError::UnexpectedData(byte)` — the peer sent a byte instead of closing.
- `ProbeError::InvalidFrame` — the peer sent a frame that violates the published
  contract, including a declared body above the limit.
- `ProbeError::Connect` / `Write` / `Read` — a socket operation failed.

Loopback tests in `tests/loopback.rs` exercise these paths over real TCP and
need no Docker.
