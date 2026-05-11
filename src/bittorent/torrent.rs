use std::fs;

use anyhow::Result;

use crate::bittorent::{encoding::Bencoding, sha1_hash};

#[allow(unused)]
pub struct Info {
    pub length: u64,
    pub name: String,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub hash: [u8; 20],
}

impl Info {
    pub fn decode(bencoded: Bencoding) -> Result<Self> {
        let info_hash = sha1_hash(&bencoded.encode());
        let Bencoding::Dictionary(mut dict) = bencoded else {
            anyhow::bail!("info must be encode as dictionary")
        };
        let Some(Bencoding::Integer(length)) = dict.remove("length") else {
            anyhow::bail!("length must be encode as integer")
        };
        let Some(Bencoding::Integer(plength)) = dict.remove("piece length") else {
            anyhow::bail!("piece length must be encode as integer")
        };
        let Some(Bencoding::String(name)) = dict.remove("name") else {
            anyhow::bail!("name must be encode as string")
        };
        let Some(Bencoding::String(pieces)) = dict.remove("pieces") else {
            anyhow::bail!("pieces must be encode as string")
        };
        let pieces: Result<Vec<_>> = pieces
            .chunks(20)
            .map(|chunk| {
                chunk.try_into().map_err(|_| {
                    anyhow::Error::msg("failed to split pieces into set of 20 bytes array")
                })
            })
            .collect();
        Ok(Info {
            name: String::from_utf8(name)?,
            pieces: pieces?,
            piece_length: plength as u64,
            length: length as u64,
            hash: info_hash,
        })
    }
}

pub struct Torrent {
    pub announce: String,
    pub info: Info,
}

impl Torrent {
    pub fn from_file(file_path: &str) -> Result<Self> {
        let data = fs::read(file_path)?;
        let Bencoding::Dictionary(mut dict) = Bencoding::decode(data)? else {
            anyhow::bail!("torrent must be encode as dictionary");
        };
        let Some(Bencoding::String(announce)) = dict.remove("announce") else {
            anyhow::bail!("announce must be encode as string")
        };
        let Some(bencoded) = dict.remove("info") else {
            anyhow::bail!("info not found")
        };
        let info = Info::decode(bencoded)?;
        Ok(Self {
            announce: String::from_utf8(announce)?,
            info,
        })
    }
}
