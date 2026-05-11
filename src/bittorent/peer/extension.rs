use std::collections::BTreeMap;

use anyhow::Result;

use crate::bittorent::encoding::Bencoding;

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

pub struct ExtensionMetadata {
    pub ut_metadata: u8,
    pub ut_pex: Option<u8>,
}

#[allow(unused)]
impl ExtensionMetadata {
    pub fn new(ut_metadata: u8, ut_pex: Option<u8>) -> Self {
        Self {
            ut_metadata,
            ut_pex,
        }
    }
}

pub struct ExtensionMessagePayload {
    pub msg_type: Option<ExtensionMessageType>,
    pub piece: Option<u32>,
    pub metadata: Option<ExtensionMetadata>,
}

#[allow(unused)]
impl ExtensionMessagePayload {
    pub fn new(
        msg_type: Option<ExtensionMessageType>,
        piece: Option<u32>,
        metadata: Option<ExtensionMetadata>,
    ) -> Self {
        Self {
            msg_type,
            piece,
            metadata,
        }
    }
}

pub struct ExtensionMessage {
    pub id: u8,
    pub payload: ExtensionMessagePayload,
}

impl ExtensionMessage {
    pub fn new(id: u8, msg_type: ExtensionMessageType, piece: u32) -> Self {
        Self {
            id,
            payload: ExtensionMessagePayload {
                metadata: None,
                msg_type: Some(msg_type),
                piece: Some(piece),
            },
        }
    }

    pub fn new_handshake_msg(id: u8, ut_metadata: u8, ut_pex: Option<u8>) -> Self {
        Self {
            id,
            payload: ExtensionMessagePayload {
                metadata: Some(ExtensionMetadata {
                    ut_metadata,
                    ut_pex,
                }),
                msg_type: None,
                piece: None,
            },
        }
    }

    pub fn encode(self) -> Vec<u8> {
        let mut bytes = vec![self.id];
        let mut dict = BTreeMap::new();
        if let Some(msg_type) = self.payload.msg_type {
            dict.insert(
                String::from("msg_type"),
                Bencoding::Integer(msg_type as i64),
            );
        }
        if let Some(piece) = self.payload.piece {
            dict.insert(String::from("piece"), Bencoding::Integer(piece as i64));
        }
        if let Some(metadata) = self.payload.metadata {
            let mut m = BTreeMap::new();
            m.insert(
                String::from("ut_metadata"),
                Bencoding::Integer(metadata.ut_metadata as i64),
            );
            if let Some(ut_pex) = metadata.ut_pex {
                m.insert(String::from("ut_pex"), Bencoding::Integer(ut_pex as i64));
            };
            dict.insert(String::from("m"), Bencoding::Dictionary(m));
        }
        bytes.extend(Bencoding::Dictionary(dict).encode());
        bytes
    }

    pub fn decode(bytes: Vec<u8>) -> Result<Self> {
        anyhow::ensure!(bytes.len() > 0);
        let id = bytes[0];
        let Bencoding::Dictionary(mut dict) = Bencoding::decode(bytes[1..].to_owned())? else {
            anyhow::bail!("parse failed")
        };
        let msg_type = if let Some(Bencoding::Integer(i)) = dict.remove("msg_type") {
            ExtensionMessageType::try_from(i).ok()
        } else {
            None
        };
        let piece = if let Some(Bencoding::Integer(i)) = dict.remove("piece") {
            Some(i as u32)
        } else {
            None
        };
        let metadata = if let Some(Bencoding::Dictionary(mut metadata)) = dict.remove("m") {
            let Some(Bencoding::Integer(ut_metadata)) = metadata.remove("ut_metadata") else {
                anyhow::bail!("ut_metadata not found")
            };
            let ut_pex = match metadata.remove("ut_pex") {
                Some(Bencoding::Integer(ut_pex)) => Some(ut_pex as u8),
                _ => None,
            };
            Some(ExtensionMetadata {
                ut_metadata: ut_metadata as u8,
                ut_pex,
            })
        } else {
            None
        };
        Ok(Self {
            id,
            payload: ExtensionMessagePayload {
                metadata,
                msg_type,
                piece,
            },
        })
    }
}
