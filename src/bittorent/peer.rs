mod extension;
mod message;

use std::{fs::OpenOptions, io::Write, sync::Arc, time::Duration};

use anyhow::Result;
use rand::{RngExt, distr::Alphanumeric};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Mutex, mpsc},
    time::sleep,
};

use crate::bittorent::{
    encoding::Bencoding,
    peer::{
        extension::{ExtensionMessage, ExtensionMessageType, ExtensionMetadata},
        message::{Message, MessageId, send_interested, wait_for_bitfield, wait_for_unchoke},
    },
    sha1_hash,
    torrent::Torrent,
};

pub const BLOCK_SIZE: u32 = 16 * 1024;

#[allow(clippy::too_many_arguments)]
pub async fn discover_peers(
    url: &str,
    info_hash: &[u8],
    port: u16,
    uploaded: u32,
    downloaded: u32,
    left: u64,
    compact: bool,
) -> Result<(u64, Vec<String>)> {
    let peer_id = generate_peer_id();
    let url = format!(
        "{}?info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact={}",
        url,
        url_encode(info_hash).as_str(),
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
    let peer_id = generate_peer_id();
    let mut buf = Vec::new();
    buf.push(19);
    buf.extend(b"BitTorrent protocol");
    buf.extend([0u8; 8]);
    buf.extend(info_hash);
    buf.extend(peer_id.into_bytes());
    stream.write_all(&buf).await?;

    let mut buf = [0u8; 68];
    stream.read_exact(&mut buf).await?;
    anyhow::ensure!(buf[0] == 19);
    anyhow::ensure!(&buf[1..20] == b"BitTorrent protocol");
    anyhow::ensure!(&buf[28..48] == info_hash);

    Ok(buf[48..].to_owned())
}

pub async fn establish_peers(
    addrs: &[Arc<String>],
    info_hash: Arc<[u8]>,
    max_concurent_peers: usize,
    timeout: u64,
) -> Vec<TcpStream> {
    let max_concurent_peers = max_concurent_peers.min(addrs.len());
    let (tx, mut rx) = mpsc::channel(max_concurent_peers);
    let mut handles = Vec::new();
    println!("establishing peers...");
    for addr in addrs {
        let tx = tx.clone();
        let addr = addr.clone();
        let info_hash = info_hash.clone();
        handles.push(tokio::spawn(async move {
            if let Ok(peer) = establish_peer(addr, info_hash).await {
                let _ = tx.send(peer).await;
            };
        }));
    }
    let mut peers = Vec::new();
    let timeout_future = sleep(Duration::from_secs(timeout));
    tokio::pin!(timeout_future);
    loop {
        tokio::select! {
            peer = rx.recv() => {
                if let Some(peer) = peer {
                    peers.push(peer);
                }
                if peers.len() == max_concurent_peers {
                    break;
                }
            }
            _ = &mut timeout_future => {
                println!("timeout after {timeout}s");
                break;
            }
        }
    }
    for handle in handles {
        handle.abort();
    }
    println!("established {} peers.", peers.len());
    peers
}

async fn establish_peer(addr: Arc<String>, info_hash: Arc<[u8]>) -> Result<TcpStream> {
    let mut stream = TcpStream::connect(addr.as_ref()).await?;
    hanshake(&mut stream, &info_hash).await?;
    wait_for_bitfield(&mut stream).await?;
    send_interested(&mut stream).await?;
    wait_for_unchoke(&mut stream).await?;
    Ok(stream)
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
                    .unwrap_or_else(|e| panic!("Error: {e}"));
                let mut piece = piece.lock().await;
                (*piece)[offset as usize..(offset + length) as usize].copy_from_slice(&data);
                println!(
                    "Downloaded from piece {}: {} bytes, offset {}",
                    piece_index, length, offset
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

    let checksum = sha1_hash(&piece_data);
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

pub async fn extension_hanshake(
    stream: &mut TcpStream,
    info_hash: &[u8],
) -> Result<(Vec<u8>, ExtensionMetadata)> {
    let peer_id = generate_peer_id();
    let mut buf = Vec::new();
    buf.push(19);
    buf.extend(b"BitTorrent protocol");
    buf.extend([0, 0, 0, 0, 0, 0x10, 0, 0]);
    buf.extend(info_hash);
    buf.extend(peer_id.into_bytes());
    stream.write_all(&buf).await?;

    let mut buf = [0u8; 68];
    stream.read_exact(&mut buf).await?;
    anyhow::ensure!(buf[0] == 19);
    anyhow::ensure!(&buf[1..20] == b"BitTorrent protocol");
    anyhow::ensure!(&buf[28..48] == info_hash);

    let reserved = &buf[20..28];
    anyhow::ensure!(reserved[5] == 0x10, "peer does not support extension");

    wait_for_bitfield(stream).await?;

    let payload = ExtensionMessage::new_handshake_msg(0, 1, None).encode();
    let ext_msg = Message::new(MessageId::Extension, payload);
    stream.write_all(&ext_msg.into_bytes()).await?;

    let ext_msg_back = Message::from_stream(stream).await?;
    anyhow::ensure!(ext_msg_back.id == MessageId::Extension);
    let ext_msg_back = ExtensionMessage::decode(ext_msg_back.payload)?;
    let Some(metadata) = ext_msg_back.payload.metadata else {
        anyhow::bail!("metadata not found")
    };
    Ok((buf[48..].to_owned(), metadata))
}

pub async fn extension_meatadata(
    stream: &mut TcpStream,
    metadata: &ExtensionMetadata,
) -> Result<()> {
    let ext_msg = ExtensionMessage::new(metadata.ut_metadata, ExtensionMessageType::Request, 0);
    let msg = Message::new(MessageId::Extension, ext_msg.encode());
    stream.write_all(&msg.into_bytes()).await?;
    Ok(())
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
