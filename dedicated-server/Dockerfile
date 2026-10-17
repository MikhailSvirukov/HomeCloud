# syntax=docker/dockerfile:1.7

FROM rust:1.99-bookworm AS build

WORKDIR /src

COPY dedicated-server/Cargo.toml dedicated-server/Cargo.lock ./

RUN mkdir src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release --locked --bin dedicated-server

COPY dedicated-server/src ./src

RUN touch src/main.rs \
    && cargo build --release --locked --bin dedicated-server

FROM debian:bookworm-slim AS runtime

RUN groupadd --gid 10001 homecloud \
    && useradd --uid 10001 --gid homecloud --no-create-home \
        --home-dir /nonexistent --shell /usr/sbin/nologin homecloud \
    && install -d -o homecloud -g homecloud /etc/homecloud

COPY --from=build /src/target/release/dedicated-server /usr/local/bin/dedicated-server

ENV HOMECLOUD_DEDICATED_SERVER_CONFIG=/etc/homecloud/dedicated-server.toml

USER homecloud

ENTRYPOINT ["/usr/local/bin/dedicated-server"]
