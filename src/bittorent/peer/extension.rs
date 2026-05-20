use std::collections::BTreeMap;

use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::bittorent::{
    encoding::Bencoding,
    peer::{
        generate_peer_id,
        message::{Message, MessageId},
        wait_for_bitfield,
    },
    torrent::Info,
};

pub struct ExtensionHandshakeMeta {
    pub ut_metadata: u8,
    pub ut_pex: Option<u8>,
}

pub struct ExtensionHandshakeMessage {
    pub id: u8,
    pub m: ExtensionHandshakeMeta,
}

impl ExtensionHandshakeMessage {
    pub fn new(id: u8, ut_metadata: u8, ut_pex: Option<u8>) -> Self {
        Self {
            id,
            m: ExtensionHandshakeMeta {
                ut_metadata,
                ut_pex,
            },
        }
    }

    pub fn encode(self) -> Vec<u8> {
        let mut bytes = vec![self.id];
        let mut dict = BTreeMap::new();
        let mut m = BTreeMap::new();
        m.insert(
            String::from("ut_metadata"),
            Bencoding::Integer(self.m.ut_metadata as i64),
        );
        if let Some(ut_pex) = self.m.ut_pex {
            m.insert(String::from("ut_pex"), Bencoding::Integer(ut_pex as i64));
        };
        dict.insert(String::from("m"), Bencoding::Dictionary(m));
        bytes.extend(Bencoding::Dictionary(dict).encode());
        bytes
    }

    pub fn decode(bytes: Vec<u8>) -> Result<Self> {
        anyhow::ensure!(bytes.len() > 0);
        let id = bytes[0];
        let Bencoding::Dictionary(mut dict) = Bencoding::decode(bytes[1..].to_owned())? else {
            anyhow::bail!("parse failed")
        };
        let Some(Bencoding::Dictionary(mut metadata)) = dict.remove("m") else {
            anyhow::bail!("'m' field not found")
        };
        let Some(Bencoding::Integer(ut_metadata)) = metadata.remove("ut_metadata") else {
            anyhow::bail!("ut_metadata not found")
        };
        let ut_pex = match metadata.remove("ut_pex") {
            Some(Bencoding::Integer(ut_pex)) => Some(ut_pex as u8),
            _ => None,
        };
        Ok(Self {
            id,
            m: ExtensionHandshakeMeta {
                ut_metadata: ut_metadata as u8,
                ut_pex,
            },
        })
    }
}

pub enum ExtensionMessageType {
    Request = 0,
    Data = 1,
    Reject = 2,
}

impl TryFrom<u8> for ExtensionMessageType {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Request),
            1 => Ok(Self::Data),
            2 => Ok(Self::Reject),
            _ => anyhow::bail!("not an extesion message"),
        }
    }
}

pub struct ExtensionMessage {
    pub id: u8,
    pub msg_type: ExtensionMessageType,
    pub piece: u32,
    pub payload: Vec<u8>,
}

impl ExtensionMessage {
    pub fn new(id: u8, msg_type: ExtensionMessageType, piece: u32, payload: Vec<u8>) -> Self {
        Self {
            id,
            msg_type,
            piece,
            payload,
        }
    }

    pub fn encode(self) -> Vec<u8> {
        let mut bytes = vec![self.id];
        let mut dict = BTreeMap::new();
        dict.insert(
            String::from("msg_type"),
            Bencoding::Integer(self.msg_type as i64),
        );
        dict.insert(String::from("piece"), Bencoding::Integer(self.piece as i64));
        bytes.extend(Bencoding::Dictionary(dict).encode());
        bytes.extend(self.payload);
        bytes
    }

    pub fn decode(bytes: Vec<u8>) -> Result<Self> {
        anyhow::ensure!(bytes.len() > 0);
        let id = bytes[0];
        let Bencoding::Dictionary(mut dict) = Bencoding::decode(bytes[1..].to_owned())? else {
            anyhow::bail!("failed to parse bencode dictionary")
        };
        let Some(Bencoding::Integer(i)) = dict.remove("msg_type") else {
            anyhow::bail!("failed to get msg_type")
        };
        let msg_type = ExtensionMessageType::try_from(i as u8)?;
        let Some(Bencoding::Integer(i)) = dict.remove("piece") else {
            anyhow::bail!("failed to get piece id")
        };
        let piece = i as u32;
        let Some(Bencoding::Integer(i)) = dict.remove("total_size") else {
            anyhow::bail!("failed to get total_size")
        };
        let total_size = i as usize;
        let payload = bytes[bytes.len() - total_size..].to_owned();
        Ok(Self {
            id,
            msg_type,
            piece,
            payload,
        })
    }
}

pub async fn hanshake(
    stream: &mut TcpStream,
    info_hash: &[u8],
) -> Result<(Vec<u8>, ExtensionHandshakeMeta)> {
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

    let payload = ExtensionHandshakeMessage::new(0, 1, None).encode();
    let ext_msg = Message::new(MessageId::Extension, payload);
    stream.write_all(&ext_msg.into_bytes()).await?;

    let ext_msg_back = Message::from_stream(stream).await?;
    anyhow::ensure!(ext_msg_back.id == MessageId::Extension);
    let ext_msg_back = ExtensionHandshakeMessage::decode(ext_msg_back.payload)?;
    Ok((buf[48..].to_owned(), ext_msg_back.m))
}

pub async fn request_torrent_info(
    stream: &mut TcpStream,
    metadata: &ExtensionHandshakeMeta,
) -> Result<Info> {
    let ext_msg = ExtensionMessage::new(
        metadata.ut_metadata,
        ExtensionMessageType::Request,
        0,
        Vec::new(),
    );
    let msg = Message::new(MessageId::Extension, ext_msg.encode());
    stream.write_all(&msg.into_bytes()).await?;
    let msg_back = Message::from_stream(stream).await?;
    let ext_msg_back = ExtensionMessage::decode(msg_back.payload)?;
    let bvalue = Bencoding::decode(ext_msg_back.payload)?;
    let info = Info::decode(bvalue)?;
    Ok(info)
}
