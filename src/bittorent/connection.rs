use std::{io::Read, net::TcpStream};

use anyhow::Result;
use rand::{RngExt, distr::Alphanumeric};

use crate::bittorent::{encoding::Bencoding, metainfo::MetaInfo};

#[derive(Debug)]
pub struct Handshake {
    pub protocol: String,
    pub reserved: [u8; 8],
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

impl Handshake {
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            protocol: String::from("BitTorrent protocol"),
            reserved: [0; 8],
            info_hash,
            peer_id,
        }
    }

    pub fn from_peer(peer: &mut TcpStream) -> Result<Self> {
        let mut plength = [0u8];
        peer.read_exact(&mut plength)?;
        let mut protocol = vec![0u8; plength[0] as usize];
        let mut reserved = [0u8; 8];
        let mut info_hash = [0u8; 20];
        let mut peer_id = [0u8; 20];
        peer.read_exact(&mut protocol)?;
        peer.read_exact(&mut reserved)?;
        peer.read_exact(&mut info_hash)?;
        peer.read_exact(&mut peer_id)?;
        Ok(Self {
            protocol: String::from_utf8(protocol)?,
            reserved,
            info_hash,
            peer_id,
        })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.push(self.protocol.len() as u8);
        bytes.extend(self.protocol.into_bytes());
        bytes.extend(self.reserved);
        bytes.extend(self.info_hash);
        bytes.extend(self.peer_id);
        bytes
    }
}

pub fn peers_discovery(metainfo: &MetaInfo, peer_id: &str) -> Result<(u64, Vec<String>)> {
    let client = reqwest::blocking::Client::new();
    let url = format!(
        "{}?info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact={}",
        metainfo.announce,
        url_encode(&metainfo.info.hash).as_str(),
        peer_id,
        "6881",
        "0",
        "0",
        metainfo.info.length.to_string().as_str(),
        "1"
    );
    let res = client.get(&url).send()?.bytes()?.to_vec();
    let data = Bencoding::decode(res)?;
    let Bencoding::Dictionary(dict) = data else {
        anyhow::bail!("tracker response must be a dictionary")
    };
    let Some(Bencoding::Integer(interval)) = dict.get("interval") else {
        anyhow::bail!("failed to parse tracker response");
    };
    let Some(Bencoding::String(peers)) = dict.get("peers") else {
        anyhow::bail!("failed to parse tracker response");
    };
    let interval = *interval as u64;
    let peers: Vec<_> = peers
        .chunks(6)
        .map(|addr| {
            let port = u16::from_be_bytes([addr[4], addr[5]]);
            format!("{}.{}.{}.{}:{}", addr[0], addr[1], addr[2], addr[3], port)
        })
        .collect();
    Ok((interval, peers))
}

pub fn random_peer_id() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(20)
        .map(char::from)
        .collect()
}

fn url_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| format!("%{}", hex::encode([b])))
        .collect::<Vec<_>>()
        .join("")
}
