# Build from the repository root:
#   docker build -f distributed-server/Dockerfile .
FROM rust:1.99-bookworm AS builder

WORKDIR /build/distributed-server

COPY distributed-server/Cargo.toml distributed-server/Cargo.lock ./
COPY distributed-server/crates ./crates

RUN cargo build --release --locked -p storage-node

FROM debian:bookworm-slim

RUN groupadd --gid 10001 storage \
    && useradd --uid 10001 --gid storage --create-home --home-dir /home/storage \
        --shell /usr/sbin/nologin storage

COPY --from=builder /build/distributed-server/target/release/storage-node \
    /usr/local/bin/storage-node
COPY distributed-server/entrypoint.sh /usr/local/bin/storage-node-entrypoint

RUN chmod 0555 /usr/local/bin/storage-node /usr/local/bin/storage-node-entrypoint

USER storage
WORKDIR /home/storage

STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/storage-node-entrypoint"]
