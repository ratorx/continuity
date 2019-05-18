FROM rust:stretch

RUN USER=root cargo new --bin continuity
WORKDIR /continuity

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

RUN mkdir -p src/torrent src/continuity && mv src/main.rs src/continuity/bin.rs && touch src/torrent/lib.rs
RUN cargo build --release
RUN rm -rf src/*
RUN rm ./target/release/deps/continuity* ./target/release/deps/torrent* ./target/release/deps/libtorrent*

COPY start.sh .
ENTRYPOINT ["/bin/sh", "./start.sh"]

COPY ./src ./src
RUN cargo build --release

