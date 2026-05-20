use std::{fs::OpenOptions, io::Write, sync::Arc, time::Duration};

use anyhow::Result;
use tokio::{
    sync::{Mutex, mpsc},
    time::sleep,
};

use crate::bittorent::{peer::Peer, sha1_hash, torrent::Info};

const BLOCK_SIZE: u32 = 16 * 1024;
const MAX_CONNECTING_PEERS: usize = 24;
const ESTABLISH_PEER_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Downloader {
    pub addrs: Vec<String>,
    pub peers: Vec<Arc<Mutex<Peer>>>,
    pub info: Info,
}

impl Downloader {
    pub fn new(addrs: Vec<String>, info: Info) -> Self {
        Self {
            addrs,
            peers: Vec::new(),
            info,
        }
    }

    pub async fn establish_peers(&mut self) {
        let total_addrs = self.addrs.len();
        let (tx, mut rx) = mpsc::channel(total_addrs);
        let mut handles = Vec::new();
        println!("Establishing peers...");
        for addr in self.addrs.iter() {
            let tx = tx.clone();
            let addr = addr.clone();
            let info_hash = self.info.hash;
            handles.push(tokio::spawn(async move {
                if let Ok(peer) = Peer::establish(addr, info_hash).await {
                    let _ = tx.send(peer).await;
                };
            }));
        }
        let mut peers = Vec::new();
        let timeout_future = sleep(ESTABLISH_PEER_TIMEOUT);
        tokio::pin!(timeout_future);
        loop {
            tokio::select! {
                peer = rx.recv() => {
                    if let Some(peer) = peer {
                        peers.push(peer);
                    }
                    if peers.len() == total_addrs.min(MAX_CONNECTING_PEERS) {
                        break;
                    }
                }
                _ = &mut timeout_future => {
                    println!("Timeout after {ESTABLISH_PEER_TIMEOUT:?}");
                    break;
                }
            }
        }
        for handle in handles {
            handle.abort();
        }
        println!("Established {} peers.", peers.len());
        self.peers = peers
            .into_iter()
            .map(|peer| Arc::new(Mutex::new(peer)))
            .collect();
    }

    pub async fn download_piece(
        &mut self,
        piece_index: u32,
        output: Option<&String>,
    ) -> Result<()> {
        let Some(piece_hash) = self.info.pieces.get(piece_index as usize) else {
            anyhow::bail!("piece_index out of range");
        };
        let piece_length = self
            .info
            .length
            .saturating_sub(self.info.piece_length * piece_index as u64)
            .min(self.info.piece_length) as u32;

        let mut number_of_blocks = piece_length / BLOCK_SIZE;
        if number_of_blocks * BLOCK_SIZE < piece_length {
            number_of_blocks += 1;
        }

        let blocks = Arc::new(Mutex::new(vec![vec![]; number_of_blocks as usize]));
        let states = Arc::new(Mutex::new(vec![false; number_of_blocks as usize]));
        let downloaded = Arc::new(Mutex::new(0));
        let mut handles = Vec::new();

        for peer in self.peers.iter() {
            let peer = peer.clone();
            let blocks = blocks.clone();
            let states = states.clone();
            let downloaded = downloaded.clone();
            handles.push(tokio::spawn(async move {
                let mut peer = peer.lock().await;
                if peer.drop || !peer.pieces[piece_index as usize] {
                    return;
                }
                loop {
                    let downloaded_guard = downloaded.lock().await;
                    if *downloaded_guard == number_of_blocks {
                        return;
                    }
                    drop(downloaded_guard);
                    let mut state_guard = states.lock().await;
                    let Some(idx) = state_guard.iter().position(|pending| !pending) else {
                        continue;
                    };
                    state_guard[idx] = true;
                    drop(state_guard);
                    let offset = idx as u32 * BLOCK_SIZE;
                    let length = (piece_length - offset).min(BLOCK_SIZE);
                    match peer.download_piece_block(piece_index, offset, length).await {
                        Ok((_, data)) => {
                            let mut blocks_guard = blocks.lock().await;
                            (*blocks_guard)[idx] = data;
                            let mut downloaded_guard = downloaded.lock().await;
                            *downloaded_guard += 1;
                        }
                        Err(e) => {
                            println!("Failed to download piece block: {e}");
                            println!("Dropped peer: {:?}", peer.addr);
                            peer.drop = true;
                            let mut state_guard = states.lock().await;
                            state_guard[idx] = false;
                            return;
                        }
                    }
                }
            }));
        }

        for handle in handles {
            handle.await?;
        }

        let piece: Vec<_> = Arc::try_unwrap(blocks)
            .map_err(|_| anyhow::Error::msg("failed to unwrap block"))?
            .into_inner()
            .into_iter()
            .flatten()
            .collect();

        let checksum = sha1_hash(&piece);
        anyhow::ensure!(&checksum == piece_hash, "checksum miss match");

        let output = output.unwrap_or(&self.info.name);
        let mut file = OpenOptions::new().create(true).append(true).open(output)?;
        file.write_all(&piece)?;

        self.retain_peers().await;
        Ok(())
    }

    async fn retain_peers(&mut self) {
        let mut peers = Vec::new();
        for peer in self.peers.iter() {
            if !peer.lock().await.drop {
                peers.push(peer.clone())
            }
        }
        self.peers = peers
    }
}
