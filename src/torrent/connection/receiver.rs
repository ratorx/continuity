use super::{Command, State};
use crate::metainfo::Metainfo;
use crate::peer::{self, Handshake, Message};
use crate::storage::PieceStore;
use bitvec::BitVec;
use failure::Fail;
use log::{self, debug, error, info, warn};
use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time;

struct Chunk {
    begin: u32,
    data: Vec<u8>,
}

#[derive(Debug, Fail)]
enum PieceBuilderError {
    #[fail(display = "Invalid chunk size (expected: {}, actual: {})", _0, _1)]
    InvalidSize(u32, u32),
    #[fail(display = "Invalid chunk begin (size: {}, begin: {})", _0, _1)]
    InvalidBegin(u32, u32),
    #[fail(
        display = "Invalid chunk (piece size: {}, chunk begin: {}, chunk size: {})",
        _0, _1, _2
    )]
    InsufficientSize(u32, u32, u32),
    #[fail(display = "Info hash does not match")]
    InvalidPiece,
}

pub struct PieceBuilder {
    metainfo: Arc<Metainfo>,
    index: u32,
    remaining: u32,
    size: u32,
    chunks: Vec<Chunk>,
}

impl PieceBuilder {
    fn new(metainfo: Arc<Metainfo>, index: u32) -> Self {
        let size = metainfo.get_piece_size(index);
        Self {
            metainfo,
            index,
            remaining: size,
            size,
            chunks: Vec::new(),
        }
    }
    fn add(&mut self, chunk: Chunk) -> Result<Option<Vec<u8>>, PieceBuilderError> {
        if chunk.data.len() > self.remaining as usize {
            return Err(PieceBuilderError::InvalidSize(
                self.remaining,
                chunk.data.len() as u32,
            ));
        } else if chunk.begin >= self.size {
            return Err(PieceBuilderError::InvalidBegin(self.size, chunk.begin));
        } else if chunk.data.len() as u32 + chunk.begin > self.size {
            return Err(PieceBuilderError::InsufficientSize(
                self.size,
                chunk.begin,
                chunk.data.len() as u32,
            ));
        } else if self.size == self.remaining && chunk.data.len() as u32 == self.remaining {
            return Ok(Some(chunk.data));
        }

        self.remaining -= chunk.data.len() as u32;
        self.chunks.push(chunk);
        if self.remaining == 0 {
            let mut s = io::Cursor::new(vec![0; self.size as usize]);
            for chunk in self.chunks.iter() {
                s.set_position(chunk.begin.into());
                s.write_all(&chunk.data).unwrap();
            }

            let v = s.into_inner();
            if !self.metainfo.verify_piece(self.index, &v) {
                return Err(PieceBuilderError::InvalidPiece);
            }
            return Ok(Some(v));
        }

        Ok(None)
    }
}

#[derive(Fail, Debug)]
enum ReceiverError {
    #[fail(display = "channel closed")]
    Channel,
    #[fail(display = "duplicate bitfield")]
    DuplicateBitfield,
    #[fail(display = "invalid handshake")]
    InvalidHandshake,
    #[fail(display = "invalid index {}", _0)]
    InvalidIndex(u32),
    #[fail(display = "invalid piece {}", _0)]
    InvalidPiece(#[cause] PieceBuilderError),
    #[fail(display = "message parsing error: {}", _0)]
    Message(#[cause] peer::Error),
}

impl From<peer::Error> for ReceiverError {
    fn from(e: peer::Error) -> Self {
        ReceiverError::Message(e)
    }
}

pub struct Receiver {
    pub tx: mpsc::Sender<Command>,
    pub piece_buffer: HashMap<u32, PieceBuilder>,
    pub state: Arc<RwLock<State>>,
    pub store: Arc<RwLock<PieceStore>>,
    pub speed: Mutex<Option<time::Duration>>,
    pub availability: Arc<Mutex<BitVec>>,
    pub reader: BufReader<TcpStream>,
    pub peer_id: Arc<String>,
    pub client_id: Arc<String>,
    pub metainfo: Arc<Metainfo>,
    pub bitfield_received: bool,
    pub num_downloaded: Arc<Mutex<u64>>,
}

impl Receiver {
    fn _start(&mut self) -> Result<(), ReceiverError> {
        // Receive Handshake
        if !Handshake::recv(
            &self.metainfo.info_hash().unwrap(),
            self.client_id.as_bytes(),
            self.reader.by_ref(),
        ) {
            return Err(ReceiverError::InvalidHandshake);
        }

        // Parse messages in loop
        loop {
            let m = Message::recv(&mut self.reader)?;

            debug!("Message received from {:?}: {:?}", self.peer_id, &m);
            match m {
                Message::KeepAlive => {
                    continue;
                }
                Message::Choke => self.choke(true)?,
                Message::Unchoke => self.choke(false)?,
                Message::Interested => self.interest(true),
                Message::NotInterested => self.interest(false),
                Message::Have(index) => self.have(index)?,
                Message::Request(index, begin, length) => self.request(index, begin, length)?,
                Message::BitField(bv) => self.bitfield(bv)?,
                Message::Piece(index, begin, piece) => self.piece(
                    index,
                    begin,
                    Arc::try_unwrap(piece).expect("Piece only has one owner"),
                )?,
                Message::Cancel(_, _, _) => continue,
                Message::Port(_) => continue, // unhandled
            }
        }
    }

    pub fn start(mut self) {
        match self._start() {
            Err(e) => warn!("{}: {}", self.peer_id, e),
            _ => unreachable!(),
        }

        // Attempt to inform sender
        let _ = self.tx.send(Command::Shutdown);
    }

    fn send_command(&self, cmd: Command) -> Result<(), ReceiverError> {
        debug!("Peer {}: Send {:?} to receiver", self.peer_id, cmd);
        self.tx.send(cmd).map_err(|_| ReceiverError::Channel)
    }

    fn choke(&mut self, state: bool) -> Result<(), ReceiverError> {
        let mut s = self.state.write().unwrap();
        (*s).peer_choked = state;
        info!("Peer {}: {:?}", self.peer_id, *s);
        drop(s);
        self.send_command(Command::PeerChoke(state))?;
        if state {
            self.piece_buffer.clear();
        }
        Ok(())
    }

    fn interest(&mut self, state: bool) {
        let mut s = self.state.write().unwrap();
        (*s).peer_interested = state;
        info!("Peer {}: {:?}", self.peer_id, *s);
    }

    fn have(&mut self, index: u32) -> Result<(), ReceiverError> {
        if index >= self.metainfo.num_pieces() {
            return Err(ReceiverError::InvalidIndex(index));
        }

        let mut bv = self.availability.lock().unwrap();
        bv.set(index as usize, true);
        drop(bv);
        self.send_command(Command::PeerHave(index))?;
        Ok(())
    }

    fn request(&mut self, index: u32, begin: u32, length: u32) -> Result<(), ReceiverError> {
        let choked = { self.state.read().unwrap().client_choked };
        if !choked {
            self.send_command(Command::SendChunk(index, begin, length))?
        }
        Ok(())
    }

    fn bitfield(&mut self, mut bv: BitVec) -> Result<(), ReceiverError> {
        if !self.bitfield_received {
            bv.truncate(self.metainfo.num_pieces() as usize);
            // Store into mutex
            let mut availability = self.availability.lock().unwrap();
            *availability = bv;
            drop(availability);
            self.send_command(Command::BitFieldReceived)?;

            self.bitfield_received = true;
            return Ok(());
        }
        Err(ReceiverError::DuplicateBitfield)
    }

    fn piece(&mut self, index: u32, begin: u32, piece: Vec<u8>) -> Result<(), ReceiverError> {
        if index >= self.metainfo.num_pieces() {
            return Err(ReceiverError::InvalidIndex(index));
        }
        // Get bitvec of items already in store
        let bv = { self.store.read().unwrap().as_bitvec(false) };
        if bv[index as usize] {
            return Err(ReceiverError::InvalidIndex(index));
        }
        self.piece_buffer.retain(|k, _| !bv[*k as usize]); // Purge completed entries
        self.piece_buffer
            .entry(index)
            .or_insert(PieceBuilder::new(self.metainfo.clone(), index));
        let pb = self.piece_buffer.get_mut(&index).unwrap();
        match pb.add(Chunk { begin, data: piece }) {
            Ok(Some(v)) => {
                let mut n = self.num_downloaded.lock().unwrap();
                *n += 1;
                drop(n);
                let mut ps = self.store.write().unwrap();
                ps.store(self.peer_id.as_str(), index, Arc::new(v));
            }
            Err(e) => return Err(ReceiverError::InvalidPiece(e)),
            _ => {}
        }
        Ok(())
    }
}
