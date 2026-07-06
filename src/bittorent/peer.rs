pub mod extension;
pub mod message;

use anyhow::Result;
use rand::{RngExt, distr::Alphanumeric};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::bittorent::{
    encoding::Bencoding,
    peer::message::{Message, MessageId},
};

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
    let Some(Bencoding::Raw(peers)) = dict.get("peers") else {
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

pub struct Peer {
    pub addr: String,
    pub stream: TcpStream,
    pub pieces: Vec<bool>,
    pub drop: bool,
}

impl Peer {
    pub async fn establish(addr: String, info_hash: [u8; 20]) -> Result<Self> {
        let mut stream = TcpStream::connect(&addr).await?;
        hanshake(&mut stream, &info_hash).await?;
        let bitfield = wait_for_bitfield(&mut stream).await?;
        send_interested(&mut stream).await?;
        wait_for_unchoke(&mut stream).await?;
        let mut pieces = Vec::with_capacity(bitfield.len() * 8);
        for byte in bitfield {
            for i in (0..8).rev() {
                if (byte >> i) & 1 == 1 {
                    pieces.push(true);
                } else {
                    pieces.push(false);
                }
            }
        }
        Ok(Self {
            addr,
            stream,
            pieces,
            drop: false,
        })
    }

    pub async fn download_piece_block(
        &mut self,
        piece_index: u32,
        offset: u32,
        length: u32,
    ) -> Result<(u32, Vec<u8>)> {
        let mut payload = Vec::new();
        payload.extend(piece_index.to_be_bytes());
        payload.extend(offset.to_be_bytes());
        payload.extend(length.to_be_bytes());

        let request = Message::new(MessageId::Request, payload);
        self.stream.write_all(&request.into_bytes()).await?;

        loop {
            let msg = Message::from_stream(&mut self.stream).await?;
            match msg.id {
                MessageId::Choke => wait_for_unchoke(&mut self.stream).await?,
                MessageId::Unchoke => {}
                MessageId::Piece => {
                    anyhow::ensure!(msg.payload.len() >= 8);
                    anyhow::ensure!(&msg.payload[..4] == piece_index.to_be_bytes());
                    anyhow::ensure!(&msg.payload[4..8] == offset.to_be_bytes());

                    let data = msg.payload[8..].to_vec();
                    return Ok((offset, data));
                }
                _ => anyhow::bail!("unexpected message id"),
            }
        }
    }
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

async fn wait_for_bitfield(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let msg = Message::from_stream(stream).await?;
    anyhow::ensure!(msg.id == MessageId::Bitfield);
    Ok(msg.payload)
}

async fn send_interested(stream: &mut TcpStream) -> Result<()> {
    let msg = Message::new(MessageId::Interested, Vec::new());
    stream.write_all(&msg.into_bytes()).await?;
    Ok(())
}

async fn wait_for_unchoke(stream: &mut TcpStream) -> Result<()> {
    let msg = Message::from_stream(stream).await?;
    anyhow::ensure!(msg.id == MessageId::Unchoke);
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
