use std::collections::BTreeMap;

use anyhow::Result;

use crate::bittorent::encoding::Bencoding;

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

impl TryFrom<i64> for ExtensionMessageType {
    type Error = anyhow::Error;
    fn try_from(value: i64) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Request),
            1 => Ok(Self::Data),
            2 => Ok(Self::Reject),
            _ => anyhow::bail!(""),
        }
    }
}

pub struct ExtensionMessage {
    pub id: u8,
    pub msg_type: ExtensionMessageType,
    pub piece: u32,
}

impl ExtensionMessage {
    pub fn new(id: u8, msg_type: ExtensionMessageType, piece: u32) -> Self {
        Self {
            id,
            msg_type,
            piece,
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
        bytes
    }

    pub fn decode_response(bytes: Vec<u8>) -> Result<(Self, Vec<u8>)> {
        anyhow::ensure!(bytes.len() > 0);
        let id = bytes[0];
        let Bencoding::Dictionary(mut dict) = Bencoding::decode(bytes[1..].to_owned())? else {
            anyhow::bail!("failed to parse bencode dictionary")
        };
        let Some(Bencoding::Integer(i)) = dict.remove("msg_type") else {
            anyhow::bail!("failed to get msg_type")
        };
        let msg_type = ExtensionMessageType::try_from(i)?;
        let Some(Bencoding::Integer(i)) = dict.remove("piece") else {
            anyhow::bail!("failed to get piece id")
        };
        let piece = i as u32;
        let Some(Bencoding::Integer(i)) = dict.remove("total_size") else {
            anyhow::bail!("failed to get total_size")
        };
        let total_size = i as usize;
        let piece_contents = bytes[bytes.len() - total_size..].to_owned();
        Ok((
            Self {
                id,
                msg_type,
                piece,
            },
            piece_contents,
        ))
    }
}
