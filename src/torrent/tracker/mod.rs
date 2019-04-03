pub mod http;
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct TorrentState {
    uploaded: u64,
    downloaded: u64,
    left: u64,
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct PeerInfo {
    #[serde(rename = "peer id")]
    id: Option<String>,
    ip: String,
    port: u16,
}
