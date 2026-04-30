mod bittorent;

use clap::Parser;

use crate::bittorent::Cli;

fn main() {
    let cli = Cli::parse();

    eprintln!("Logs from program:");

    match cli.run() {
        Ok(_) => {}
        Err(e) => eprintln!("Error: {e}"),
    }
}
