pub mod connection;
pub mod encoding;
pub mod metainfo;

use std::{io::Write, net::TcpStream};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::bittorent::{
    connection::{Handshake, peers_discovery, random_peer_id},
    encoding::Bencoding,
    metainfo::MetaInfo,
};

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(name = "decode")]
    Decode { encoded_value: String },

    #[command(name = "info")]
    Info { file_path: String },

    #[command(name = "peers")]
    Peers { file_path: String },

    #[command(name = "handshake")]
    Handshake { file_path: String, addr: String },
}

#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn run(self) -> Result<()> {
        match self.command {
            Command::Decode { encoded_value } => {
                let decoded_value = Bencoding::decode(encoded_value.into_bytes())?;
                println!("{}", decoded_value);
                Ok(())
            }
            Command::Info { file_path } => {
                let metainfo = MetaInfo::from_file(&file_path)?;
                println!("Tracker URL: {}", metainfo.announce);
                println!("Length: {}", metainfo.info.length);
                println!("Info Hash: {}", hex::encode(metainfo.info.hash));
                println!("Piece Length: {}", metainfo.info.piece_length);
                println!("Piece Hashes:");
                for piece in metainfo.info.pieces {
                    println!("{}", hex::encode(piece));
                }
                Ok(())
            }
            Command::Peers { file_path } => {
                let metainfo = MetaInfo::from_file(&file_path)?;
                let peer_id = random_peer_id();
                let (_, peers) = peers_discovery(&metainfo, &peer_id)?;
                for peer in peers {
                    println!("{peer}");
                }
                Ok(())
            }
            Command::Handshake { file_path, addr } => {
                let metainfo = MetaInfo::from_file(&file_path)?;
                let peer_id: [u8; 20] = random_peer_id().into_bytes().try_into().map_err(|_| {
                    anyhow::Error::msg("failed to convert peer_id to 20 bytes array")
                })?;
                let handshake = Handshake::new(metainfo.info.hash, peer_id);
                let mut peer = TcpStream::connect(addr)?;
                peer.write_all(&handshake.into_bytes())?;
                let h = Handshake::from_peer(&mut peer)?;
                println!("Peer ID: {}", hex::encode(h.peer_id));
                Ok(())
            }
        }
    }
}
