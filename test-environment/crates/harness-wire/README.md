# harness-wire

Test-environment conformance oracle and shared golden-vector source for the
published v1 wire contract in [`contracts/wire-v1.md`](../../../contracts/wire-v1.md).
The crate is dependency-free apart from `thiserror`; it performs no network,
cryptographic, or configuration work.

## Requirements covered

- `TST-019 / 1`, `TST-022 / 1`, `TST-030 / 1`, `TST-032 / 1` — the golden
  vectors and reserved-code classes used to verify every implementation.
- `INT-NET-002/1`, `INT-NET-003/1`, `INT-NET-004/1`, `INT-NET-005/1`,
  `INT-NET-006/2`, `INT-NET-008/2`, `INT-NET-047/2`, `INT-NET-049/1`,
  `INT-NET-050/4`, `INT-NET-061/1`, `INT-NET-062/1`, `INT-NET-063/1`,
  `INT-NET-069/1`, `INT-NET-081/1` — the frame, external header, and result
  envelope representation exercised by those vectors.

## API

- `harness_wire::cbor` — `Value`, `CborError`, `encode`, `decode`; strict
  deterministic CBOR (minimal integers, definite lengths, no maps or tags).
- `harness_wire::header` — `ExternalFrameHeader`, `HeaderError`; the
  seven-position array fixed by `INT-NET-050 / 4`.
- `harness_wire::frame` — `Frame`, `FrameError`, `encode`, `decode`,
  `signed_message`; the `[length][header][signature][payload]` layout.
- `harness_wire::result` — `ResultEnvelope`, `ResultClass`, `class_of`, the 18
  published result-code constants, and `ResultError`.

## Configuration inputs

Callers pass the limits; the crate holds no configuration of its own.

- `CFG-CL-011` frame body limit — the maximum value of the 4-byte length prefix,
  passed to `frame::encode`, `frame::decode`, and `frame::signed_message`.
- `CFG-CL-031` responder-label limit in UTF-8 bytes — passed to
  `ExternalFrameHeader::encode` and `ExternalFrameHeader::decode`.

## Consuming the vectors

`tests/golden_vectors.rs` pins the exact contract bytes for the ordinary and
primary-registration headers, the frame body and signed range, and every
published result code. Component implementations (Android, dedicated server,
distributed storage) can consume the same vectors as external conformance tests
so that one encoding is proven across languages.
