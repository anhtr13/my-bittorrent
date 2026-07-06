#!/bin/sh

set -e

(
    cd "$(dirname "$0")"
    cargo build --release --target-dir=/tmp/codecrafters-build-bittorrent-rust --manifest-path Cargo.toml
)

exec /tmp/codecrafters-build-bittorrent-rust/release/my-bittorrent "$@"
