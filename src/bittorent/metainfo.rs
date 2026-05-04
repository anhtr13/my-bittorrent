use std::fs;

use anyhow::Result;
use sha1::{Digest, Sha1};

use crate::bittorent::encoding::Bencoding;

#[allow(unused)]
pub struct Info {
    pub length: u64,
    pub name: String,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub hash: [u8; 20],
}

pub struct MetaInfo {
    pub announce: String,
    pub info: Info,
}

impl MetaInfo {
    pub fn from_file(file_path: &str) -> Result<Self> {
        let data = fs::read(file_path)?;
        let Bencoding::Dictionary(dict) = Bencoding::decode(data)? else {
            anyhow::bail!("metainfo must be encode as dictionary");
        };
        let Some(Bencoding::String(url)) = dict.get("announce") else {
            anyhow::bail!("announce must be encode as string")
        };
        let announce = String::from_utf8(url.to_owned())?;
        let Some(info) = dict.get("info") else {
            anyhow::bail!("info not found")
        };
        let mut encoder = Sha1::new();
        encoder.update(info.encode());
        let info_hash: [u8; 20] = encoder
            .finalize()
            .to_vec()
            .try_into()
            .map_err(|_| anyhow::Error::msg("failed to encode info_hash to 20 bytes array"))?;
        let Bencoding::Dictionary(dict) = info else {
            anyhow::bail!("info must be encode as dictionary")
        };
        let Some(Bencoding::Integer(length)) = dict.get("length") else {
            anyhow::bail!("length must be encode as integer")
        };
        let Some(Bencoding::Integer(plength)) = dict.get("piece length") else {
            anyhow::bail!("piece length must be encode as integer")
        };
        let Some(Bencoding::String(name)) = dict.get("name") else {
            anyhow::bail!("name must be encode as string")
        };
        let name = String::from_utf8(name.to_owned())?;
        let Some(Bencoding::String(pieces)) = dict.get("pieces") else {
            anyhow::bail!("pieces must be encode as string")
        };
        let pieces: Result<Vec<_>> = pieces
            .chunks(20)
            .map(|chunk| {
                chunk
                    .try_into()
                    .map_err(|_| anyhow::Error::msg("failed to split pieces into 20 bytes arrays"))
            })
            .collect();
        let pieces = pieces?;
        Ok(Self {
            announce,
            info: Info {
                length: *length as u64,
                name,
                piece_length: *plength as u64,
                pieces,
                hash: info_hash,
            },
        })
    }
}
