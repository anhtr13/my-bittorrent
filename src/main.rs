mod bencoding;

use std::env;

use anyhow::Result;

use crate::bencoding::Bencoding;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        eprintln!("Logs from program:");

        let encoded_value = &args[2];
        let Some(decoded_value) = Bencoding::decode(&mut encoded_value.chars())? else {
            anyhow::bail!("decode failed");
        };

        println!("{}", decoded_value);
    } else {
        println!("unknown command: {}", args[1])
    }

    Ok(())
}
