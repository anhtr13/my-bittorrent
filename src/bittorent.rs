mod downloader;
mod encoding;
mod magnet;
mod peer;
mod torrent;

use anyhow::Result;
use clap::{Parser, Subcommand};
use sha1::{Digest, Sha1};
use tokio::net::TcpStream;

use crate::bittorent::{
    downloader::Downloader,
    encoding::Bencoding,
    magnet::Magnet,
    peer::{discover_peers, extension, hanshake},
    torrent::Torrent,
};

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(name = "decode")]
    Decode { encoded_value: String },

    #[command(name = "info")]
    Info { torrent: String },

    #[command(name = "peers")]
    Peers { torrent: String },

    #[command(name = "handshake")]
    Handshake { torrent: String, addr: String },

    #[command(name = "download_piece")]
    DownloadPiece {
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
        torrent: String,
        piece_index: u32,
    },

    #[command(name = "download")]
    Download {
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
        torrent: String,
    },

    #[command(name = "magnet_parse")]
    MagnetParse { link: String },

    #[command(name = "magnet_handshake")]
    MagnetHandshake { link: String },

    #[command(name = "magnet_info")]
    MagnetInfo { link: String },

    #[command(name = "magnet_download_piece")]
    MagnetDownloadPiece {
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
        link: String,
        piece_index: u32,
    },

    #[command(name = "magnet_download")]
    MagnetDownload {
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
        link: String,
    },
}

#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Command::Decode { encoded_value } => {
                let decoded_value = Bencoding::decode(encoded_value.into_bytes())?;
                println!("{}", decoded_value);
                Ok(())
            }
            Command::Info { torrent } => {
                let torrent = Torrent::from_file(&torrent)?;
                println!("Tracker URL: {}", torrent.announce);
                println!("Length: {}", torrent.info.length);
                println!("Info Hash: {}", hex::encode(torrent.info.hash));
                println!("Piece Length: {}", torrent.info.piece_length);
                println!("Piece Hashes:");
                for piece in torrent.info.pieces {
                    println!("{}", hex::encode(piece));
                }
                Ok(())
            }
            Command::Peers { torrent } => {
                let torrent = Torrent::from_file(&torrent)?;
                let (_, addrs) = discover_peers(
                    &torrent.announce,
                    &torrent.info.hash,
                    6881,
                    0,
                    0,
                    torrent.info.length,
                    true,
                )
                .await?;
                for addr in addrs {
                    println!("{addr}");
                }
                Ok(())
            }
            Command::Handshake { torrent, addr } => {
                let torrent = Torrent::from_file(&torrent)?;
                let mut stream = TcpStream::connect(addr).await?;
                let peer_id_back = hanshake(&mut stream, &torrent.info.hash).await?;
                println!("Peer ID: {}", hex::encode(peer_id_back));
                Ok(())
            }
            Command::DownloadPiece {
                output,
                torrent,
                piece_index,
            } => {
                let torrent = Torrent::from_file(&torrent)?;
                let (_, addrs) = discover_peers(
                    &torrent.announce,
                    &torrent.info.hash,
                    6881,
                    0,
                    0,
                    torrent.info.length,
                    true,
                )
                .await?;
                let mut downloader = Downloader::new(addrs, torrent.info);
                downloader.establish_peers().await;
                downloader
                    .download_piece(piece_index, output.as_ref())
                    .await?;
                Ok(())
            }
            Command::Download { output, torrent } => {
                let torrent = Torrent::from_file(&torrent)?;
                let (_, addrs) = discover_peers(
                    &torrent.announce,
                    &torrent.info.hash,
                    6881,
                    0,
                    0,
                    torrent.info.length,
                    true,
                )
                .await?;
                let total_pieces = torrent.info.pieces.len();
                let mut downloader = Downloader::new(addrs, torrent.info);
                downloader.establish_peers().await;
                for idx in 0..total_pieces {
                    downloader
                        .download_piece(idx as u32, output.as_ref())
                        .await?;
                    println!("Downloaded {}/{} pieces", idx + 1, total_pieces);
                }
                Ok(())
            }
            Command::MagnetParse { link } => {
                let magnet = Magnet::parse(link)?;
                println!("Tracker URL: {}", magnet.tracker_url);
                println!("Info Hash: {}", hex::encode(magnet.info_hash));
                Ok(())
            }
            Command::MagnetHandshake { link } => {
                let magnet = Magnet::parse(link)?;
                let (_, addrs) = discover_peers(
                    &magnet.tracker_url,
                    &magnet.info_hash,
                    6881,
                    0,
                    0,
                    9999,
                    true,
                )
                .await?;
                let mut stream = TcpStream::connect(&addrs[0]).await?;
                let (peer_id_back, metadata) =
                    extension::hanshake(&mut stream, &magnet.info_hash).await?;
                println!("Peer ID: {}", hex::encode(peer_id_back));
                println!("Peer Metadata Extension ID: {}", metadata.ut_metadata);
                Ok(())
            }
            Command::MagnetInfo { link } => {
                let magnet = Magnet::parse(link)?;
                let (_, addrs) = discover_peers(
                    &magnet.tracker_url,
                    &magnet.info_hash,
                    6881,
                    0,
                    0,
                    9999,
                    true,
                )
                .await?;
                let mut stream = TcpStream::connect(&addrs[0]).await?;
                let (_peer_id_back, metadata) =
                    extension::hanshake(&mut stream, &magnet.info_hash).await?;
                let info = extension::request_torrent_info(&mut stream, &metadata).await?;
                println!("Tracker URL: {}", magnet.tracker_url);
                println!("Length: {}", info.length);
                println!("Info Hash: {}", hex::encode(info.hash));
                println!("Piece Length: {}", info.piece_length);
                println!("Piece Hashes:");
                for piece in info.pieces {
                    println!("{}", hex::encode(piece));
                }
                Ok(())
            }
            Command::MagnetDownloadPiece {
                output,
                link,
                piece_index,
            } => {
                let magnet = Magnet::parse(link)?;
                let (_, addrs) = discover_peers(
                    &magnet.tracker_url,
                    &magnet.info_hash,
                    6881,
                    0,
                    0,
                    9999,
                    true,
                )
                .await?;
                let mut stream = TcpStream::connect(&addrs[0]).await?;
                let (_peer_id_back, ext_handshake_meta) =
                    extension::hanshake(&mut stream, &magnet.info_hash).await?;
                let info =
                    extension::request_torrent_info(&mut stream, &ext_handshake_meta).await?;
                let (_, addrs) = discover_peers(
                    &magnet.tracker_url,
                    &info.hash,
                    6881,
                    0,
                    0,
                    info.length,
                    true,
                )
                .await?;
                let mut downloader = Downloader::new(addrs, info);
                downloader.establish_peers().await;
                downloader
                    .download_piece(piece_index, output.as_ref())
                    .await?;
                Ok(())
            }
            Command::MagnetDownload { output, link } => {
                let magnet = Magnet::parse(link)?;
                let (_, addrs) = discover_peers(
                    &magnet.tracker_url,
                    &magnet.info_hash,
                    6881,
                    0,
                    0,
                    9999,
                    true,
                )
                .await?;
                let mut stream = TcpStream::connect(&addrs[0]).await?;
                let (_peer_id_back, ext_handshake_meta) =
                    extension::hanshake(&mut stream, &magnet.info_hash).await?;
                let info =
                    extension::request_torrent_info(&mut stream, &ext_handshake_meta).await?;
                let (_, addrs) = discover_peers(
                    &magnet.tracker_url,
                    &info.hash,
                    6881,
                    0,
                    0,
                    info.length,
                    true,
                )
                .await?;
                let total_pieces = info.pieces.len();
                let mut downloader = Downloader::new(addrs, info);
                downloader.establish_peers().await;
                for idx in 0..total_pieces {
                    downloader
                        .download_piece(idx as u32, output.as_ref())
                        .await?;
                    println!("Downloaded {}/{} pieces", idx + 1, total_pieces);
                }
                Ok(())
            }
        }
    }
}

fn sha1_hash(data: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    result.into()
}
