pub mod encoding;

use std::fs;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::bittorent::encoding::Bencoding;

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(name = "decode")]
    Decode { encoded_value: String },

    #[command(name = "info")]
    Info { file_path: String },
}

#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn run(self) -> Result<()> {
        match self.command {
            Command::Decode { encoded_value } => {
                let decoded_value = Bencoding::decode(encoded_value.into_bytes())?;
                println!("{}", decoded_value.to_string());
                Ok(())
            }
            Command::Info { file_path } => {
                let data = fs::read(file_path)?;
                let Bencoding::Dictionary(dict) = Bencoding::decode(data)? else {
                    anyhow::bail!("metainfo must be a dictionary");
                };
                let Some(announce) = dict.get("announce") else {
                    anyhow::bail!("announce not found")
                };
                let Bencoding::String(url) = announce else {
                    anyhow::bail!("announce must be string")
                };
                let Some(info) = dict.get("info") else {
                    anyhow::bail!("info not found")
                };
                let Bencoding::Dictionary(info) = info else {
                    anyhow::bail!("info must be a dictionary")
                };
                let Some(length) = info.get("length") else {
                    anyhow::bail!("length not found")
                };
                let Bencoding::Integer(length) = length else {
                    anyhow::bail!("length must be an integer")
                };
                println!("Tracker URL: {}", str::from_utf8(url)?);
                println!("Length: {}", length);
                Ok(())
            }
        }
    }
}
