use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[derive(Debug, PartialEq, Clone, Copy)]
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
    Extension = 20,
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
        let length = stream.read_u32().await?;
        anyhow::ensure!(length > 0);

        let byte = stream.read_u8().await?;
        let id = MessageId::try_from(byte)?;

        let length = length as usize - 1;
        if length == 0 {
            return Ok(Self {
                id,
                payload: Vec::new(),
            });
        }

        let mut payload = vec![0u8; length];
        stream.read_exact(&mut payload).await?;

        Ok(Self { id, payload })
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
            20 => Ok(Self::Extension),
            v => anyhow::bail!("Invalid message id: {}", v),
        }
    }
}

pub async fn wait_for_bitfield(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let msg = Message::from_stream(stream).await?;
    anyhow::ensure!(msg.id == MessageId::Bitfield);
    Ok(msg.payload)
}

pub async fn send_interested(stream: &mut TcpStream) -> Result<()> {
    let msg = Message::new(MessageId::Interested, Vec::new());
    stream.write_all(&msg.into_bytes()).await?;
    Ok(())
}

pub async fn wait_for_unchoke(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let msg = Message::from_stream(stream).await?;
    anyhow::ensure!(msg.id == MessageId::Unchoke);
    Ok(msg.payload)
}
