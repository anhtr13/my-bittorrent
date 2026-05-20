# My bittorrent

A small, multi-threaded BitTorrent client that's capable of parsing .torrent files or magnet links, and downloading a file from multiple peers concurently.

Build to learn about how torrent files are structured, HTTP trackers, BitTorrent’s Peer Protocol, pipelining, etc.

Part of the ["Build Your Own BitTorrent"](https://app.codecrafters.io/courses/bittorrent/overview) challenge:
![progress-banner](https://backend.codecrafters.io/progress/bittorrent/cd5b0695-a581-4bb1-ac92-1fb5839d7850)

## Build & Run

```bash
# Release build
cargo build --release

# Download from a torrent file
./target/release/my-bittorrent download [torrent_file] -o [output?]

# Download from a magnet link
./target/release/my-bittorrent magnet_download [magnet_link] -o [output?]
```
