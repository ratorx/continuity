pub mod http;
use byteorder::{ReadBytesExt, BE};
use failure::Fail;
use serde_derive::Serialize;
use std::net::SocketAddrV4;

pub trait Discover {
    type Error;
    fn get_peers(
        &mut self,
        state: &TorrentState,
        num_peers: Option<u64>,
    ) -> Result<Vec<PeerInfo>, Self::Error>;
}

#[derive(Serialize, Debug)]
pub struct TorrentState {
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
}

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "Length not multiple of 6")]
    InvalidLength,
}

#[derive(Debug, PartialEq)]
pub struct PeerInfo {
    pub addr: SocketAddrV4,
}

impl PeerInfo {
    fn deserialize(serialized: &mut &[u8]) -> Result<Vec<Self>, Error> {
        let mut v = Vec::with_capacity(serialized.len() / 6);
        let mut to_read = serialized.len();
        while to_read != 0 {
            let ip = serialized.read_u32::<BE>().unwrap().into();
            let port = serialized.read_u16::<BE>().unwrap();
            to_read -= 6;
            v.push(PeerInfo {
                addr: SocketAddrV4::new(ip, port),
            });
        }
        Ok(v)
    }
}
