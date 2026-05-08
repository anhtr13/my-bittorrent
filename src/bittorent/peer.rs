use std::{fs::OpenOptions, io::Write, sync::Arc};

use anyhow::Result;
use rand::{RngExt, distr::Alphanumeric};
use sha1::{Digest, Sha1};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Mutex,
};

use crate::bittorent::{encoding::Bencoding, torrent::Torrent};

pub const BLOCK_SIZE: u32 = 16 * 1024;

#[allow(clippy::too_many_arguments)]
pub async fn discover_peers(
    torrent: &Torrent,
    port: u16,
    uploaded: u32,
    downloaded: u32,
    left: u64,
    compact: bool,
) -> Result<(u64, Vec<String>)> {
    let peer_id = generate_peer_id();
    let url = format!(
        "{}?info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact={}",
        torrent.announce,
        url_encode(&torrent.info.hash).as_str(),
        peer_id,
        port,
        uploaded,
        downloaded,
        left,
        compact as u8
    );
    let client = reqwest::Client::new();
    let res = client.get(&url).send().await?.bytes().await?.to_vec();
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

pub async fn hanshake(stream: &mut TcpStream, info_hash: &[u8]) -> Result<Vec<u8>> {
    let protocol = String::from("BitTorrent protocol");
    let reserved = [0u8; 8];
    let peer_id = generate_peer_id();

    let mut buf = Vec::new();
    buf.push(protocol.len() as u8);
    buf.extend(protocol.into_bytes());
    buf.extend(reserved);
    buf.extend(info_hash);
    buf.extend(peer_id.as_bytes());
    stream.write_all(&buf).await?;

    let mut buf = [0u8; 68];
    stream.read_exact(&mut buf).await?;
    anyhow::ensure!(buf[0] == 19);
    anyhow::ensure!(&buf[1..20] == b"BitTorrent protocol");
    anyhow::ensure!(&buf[28..48] == info_hash);

    Ok(buf[48..].to_owned())
}

pub async fn download_piece(
    peers: &[Arc<Mutex<TcpStream>>],
    piece_index: u32,
    torrent: &Torrent,
    output: Option<&String>,
) -> Result<()> {
    let Some(piece_hash) = torrent.info.pieces.get(piece_index as usize) else {
        anyhow::bail!("piece_index out of range");
    };
    let piece_length = torrent
        .info
        .length
        .saturating_sub(torrent.info.piece_length * piece_index as u64)
        .min(torrent.info.piece_length) as u32;

    let mut number_of_blocks = piece_length / BLOCK_SIZE;
    if number_of_blocks * BLOCK_SIZE < piece_length {
        number_of_blocks += 1;
    }

    let blocks = Arc::new(Mutex::new(vec![false; number_of_blocks as usize]));
    let piece = Arc::new(Mutex::new(vec![0u8; piece_length as usize]));
    let mut handles = Vec::new();

    for peer in peers {
        let peer = peer.clone();
        let blocks = blocks.clone();
        let piece = piece.clone();
        handles.push(tokio::spawn(async move {
            loop {
                let mut guard = blocks.lock().await;
                let Some(block_index) = guard.iter().position(|downloaded| !downloaded) else {
                    break;
                };
                guard[block_index] = true;
                drop(guard);
                let offset = block_index as u32 * BLOCK_SIZE;
                let length = (piece_length - offset).min(BLOCK_SIZE);
                let (_, data) = download_piece_block(peer.clone(), piece_index, offset, length)
                    .await
                    .unwrap_or_else(|_| {
                        panic!("failed to download block {block_index} from piece {piece_index}")
                    });
                let mut piece = piece.lock().await;
                (*piece)[offset as usize..(offset + length) as usize].copy_from_slice(&data);
                println!(
                    "Downloaded from piece {}: (block {}, offset {}, length {})",
                    piece_index, block_index, offset, length
                );
            }
        }));
    }

    for handle in handles {
        handle.await?;
    }

    let piece_data = Arc::try_unwrap(piece)
        .map_err(|_| anyhow::Error::msg("failed to get piece"))?
        .into_inner();

    let mut encoder = Sha1::new();
    encoder.update(&piece_data);
    let checksum: [u8; 20] = encoder
        .finalize()
        .to_vec()
        .try_into()
        .map_err(|_| anyhow::Error::msg("sha1 hash failed"))?;
    anyhow::ensure!(&checksum == piece_hash, "checksum miss match");

    let output = output.unwrap_or(&torrent.info.name);
    let mut file = OpenOptions::new().create(true).append(true).open(output)?;
    file.write_all(&piece_data)?;

    Ok(())
}

async fn download_piece_block(
    stream: Arc<Mutex<TcpStream>>,
    piece_index: u32,
    offset: u32,
    length: u32,
) -> Result<(u32, Vec<u8>)> {
    let mut payload = Vec::new();
    payload.extend(piece_index.to_be_bytes());
    payload.extend(offset.to_be_bytes());
    payload.extend(length.to_be_bytes());

    let request = Message::new(MessageId::Request, payload);

    let mut stream = stream.lock().await;
    stream.write_all(&request.into_bytes()).await?;

    let block = Message::from_stream(&mut stream).await?;
    anyhow::ensure!(block.id == MessageId::Piece);
    anyhow::ensure!(block.payload.len() >= 8);
    anyhow::ensure!(&block.payload[..4] == piece_index.to_be_bytes());
    anyhow::ensure!(&block.payload[4..8] == offset.to_be_bytes());

    let data = block.payload[8..].to_vec();
    Ok((offset, data))
}

pub async fn establish_peers(
    addrs: &[String],
    info_hash: &[u8],
    max_peers: usize,
) -> Vec<TcpStream> {
    let mut peers = Vec::new();
    for addr in addrs {
        if let Ok(peer) = establish_peer(addr, info_hash).await {
            peers.push(peer);
        }
        if peers.len() == max_peers {
            break;
        }
    }
    peers
}

async fn establish_peer(addr: &str, info_hash: &[u8]) -> Result<TcpStream> {
    let mut stream = TcpStream::connect(addr).await?;
    let _ = hanshake(&mut stream, info_hash).await?;

    let bitfield = Message::from_stream(&mut stream).await?;
    anyhow::ensure!(bitfield.id == MessageId::Bitfield);

    let interested = Message::new(MessageId::Interested, Vec::new());
    stream.write_all(&interested.into_bytes()).await?;

    let unchoke = Message::from_stream(&mut stream).await?;
    anyhow::ensure!(unchoke.id == MessageId::Unchoke);

    Ok(stream)
}

#[derive(PartialEq, Clone, Copy)]
pub enum MessageId {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
}

pub struct Message {
    pub id: MessageId,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(id: MessageId, payload: Vec<u8>) -> Self {
        Self { id, payload }
    }

    pub async fn from_stream(stream: &mut TcpStream) -> Result<Self> {
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await?;

        let length = u32::from_be_bytes(buf);
        anyhow::ensure!(length > 0);

        let mut id = [0u8; 1];
        stream.read_exact(&mut id).await?;

        let length = length as usize - 1;
        if length == 0 {
            return Ok(Self {
                id: MessageId::try_from(id[0])?,
                payload: Vec::new(),
            });
        }

        let mut payload = vec![0u8; length];
        stream.read_exact(&mut payload).await?;

        Ok(Self {
            id: MessageId::try_from(id[0])?,
            payload,
        })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let length = self.payload.len() as u32 + 1;
        let mut bytes = Vec::new();
        bytes.extend(length.to_be_bytes());
        bytes.push(self.id as u8);
        bytes.extend(self.payload);
        bytes
    }
}

impl TryFrom<u8> for MessageId {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Choke),
            1 => Ok(Self::Unchoke),
            2 => Ok(Self::Interested),
            3 => Ok(Self::NotInterested),
            4 => Ok(Self::Have),
            5 => Ok(Self::Bitfield),
            6 => Ok(Self::Request),
            7 => Ok(Self::Piece),
            8 => Ok(Self::Cancel),
            v => anyhow::bail!("Invalid message id: {}", v),
        }
    }
}

fn generate_peer_id() -> String {
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
