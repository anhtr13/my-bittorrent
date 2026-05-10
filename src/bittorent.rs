mod encoding;
mod magnet;
mod peer;
mod peer_message;
mod torrent;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use sha1::{Digest, Sha1};
use tokio::{net::TcpStream, sync::Mutex};

use crate::bittorent::{
    encoding::Bencoding,
    magnet::Magnet,
    peer::{discover_peers, download_piece, establish_peers, extended_hanshake, hanshake},
    torrent::Torrent,
};

const FETCH_PEER_TIMEOUT: u64 = 10;

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
                let addrs: Vec<_> = addrs.into_iter().map(Arc::new).collect();
                let peers: Vec<_> =
                    establish_peers(&addrs, Arc::new(torrent.info.hash), 8, FETCH_PEER_TIMEOUT)
                        .await
                        .into_iter()
                        .map(|peer| Arc::new(Mutex::new(peer)))
                        .collect();
                download_piece(&peers, piece_index, &torrent, output.as_ref()).await?;
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
                let addrs: Vec<_> = addrs.into_iter().map(Arc::new).collect();
                let peers: Vec<_> =
                    establish_peers(&addrs, Arc::new(torrent.info.hash), 8, FETCH_PEER_TIMEOUT)
                        .await
                        .into_iter()
                        .map(|peer| Arc::new(Mutex::new(peer)))
                        .collect();
                for idx in 0..torrent.info.pieces.len() {
                    download_piece(&peers, idx as u32, &torrent, output.as_ref()).await?;
                }
                Ok(())
            }
            Command::MagnetParse { link } => {
                let magnet_info = Magnet::parse(link)?;
                println!("Tracker URL: {}", magnet_info.tracker_url);
                println!("Info Hash: {}", hex::encode(magnet_info.info_hash));
                Ok(())
            }
            Command::MagnetHandshake { link } => {
                let magnet_info = Magnet::parse(link)?;
                let (_, addrs) = discover_peers(
                    &magnet_info.tracker_url,
                    &magnet_info.info_hash,
                    6881,
                    0,
                    0,
                    9999,
                    true,
                )
                .await?;
                let mut stream = TcpStream::connect(&addrs[0]).await?;
                let peer_id_back = extended_hanshake(&mut stream, &magnet_info.info_hash).await?;
                println!("Peer ID: {}", hex::encode(peer_id_back));
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
