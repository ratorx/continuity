use super::{Command, State};
use crate::metainfo::Metainfo;
use crate::peer::{Handshake, Message};
use crate::storage::PieceStore;
use bitvec::BitVec;
use failure::Fail;
use log::{self, debug, error, info, warn};
use std::collections::HashSet;
use std::collections::VecDeque;
use std::io::{self, BufWriter, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time;

const QUEUE_LENGTH: usize = 5;

pub struct Piece {
    index: u32,
    begin: u32,
    length: u32,
    data: Arc<Vec<u8>>,
}

impl Piece {
    fn new(index: u32, begin: u32, length: u32, data: Arc<Vec<u8>>) -> Self {
        assert!(begin < data.len() as u32);
        assert!(begin + length <= data.len() as u32);
        Piece {
            index,
            begin,
            length,
            data,
        }
    }
}

impl Into<Message> for Piece {
    fn into(self) -> Message {
        // This special case is not required, but is an optimisation
        // Important because my client only requests full pieces, so this will be only case.
        // General case included for compatibility with other clients
        if self.data.len() as u32 == self.length && self.begin == 0 {
            return Message::Piece(self.index, self.begin, self.data);
        }
        let mut v = Vec::with_capacity(self.length as usize);
        v.copy_from_slice(&self.data[self.begin as usize..(self.begin + self.length) as usize]);
        Message::Piece(self.index, self.begin, Arc::new(v))
    }
}

#[derive(Debug, Fail)]
pub enum SenderError {
    #[fail(display = "io error: {}", _0)]
    IO(#[cause] io::Error),
    #[fail(display = "channel closed")]
    Channel,
    #[fail(display = "connection shutdown")]
    Shutdown,
    #[fail(display = "invalid piece request")]
    InvalidRequest,
}

impl From<io::Error> for SenderError {
    fn from(e: io::Error) -> Self {
        SenderError::IO(e)
    }
}

pub struct Sender {
    // Queues used to handle priority 1 messages
    pub requests: VecDeque<Message>,
    pub pending: HashSet<u32>,
    pub pieces: VecDeque<Piece>,
    // Command receiver
    pub rx: mpsc::Receiver<Command>,
    // State of this connection
    pub state: Arc<RwLock<State>>,
    // Shared global store
    pub store: Arc<RwLock<PieceStore>>,
    // Used to track peer availability and limit have messages
    pub availability: Arc<Mutex<BitVec>>,
    // Shared readonly metadata
    pub metainfo: Arc<Metainfo>,
    // Peer id - used for logging
    pub peer_id: Arc<String>,
    // Client id - used for handshake
    pub client_id: Arc<String>,
    // Stream
    pub writer: BufWriter<TcpStream>,
    // Metrics exposed for seeding
    pub num_uploaded: Arc<Mutex<u64>>,
}

impl Sender {
    fn _start(&mut self) -> Result<(), SenderError> {
        Handshake::send(
            &self.metainfo.info_hash().unwrap(),
            Some(self.client_id.as_bytes()),
            self.writer.by_ref(),
        )?;

        let bv = { self.store.read().unwrap().as_bitvec(false) };
        self.send(Message::BitField(bv))?;

        'main: loop {
            self.handle_commands()?;
            self.writer.flush()?;

            match self.requests.pop_front() {
                Some(msg) => {
                    self.send(msg)?;
                }
                None => {}
            }
            if self.pending.len() <= QUEUE_LENGTH / 2 {
                debug!("Queue pieces triggered by queue length");
                self.queue_pieces()?;
            }

            self.handle_commands()?;
            self.writer.flush()?;

            match self.pieces.pop_front() {
                Some(piece) => self.send(piece.into())?,
                None => {}
            }

            if self.requests.len() == 0 && self.pieces.len() == 0 {
                loop {
                    match self.rx.recv_timeout(time::Duration::from_secs(90)) {
                        Ok(cmd) => {
                            self.handle(cmd)?;
                            continue 'main;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => self.send(Message::KeepAlive)?,
                        Err(_) => return Err(SenderError::Shutdown),
                    }
                }
            }
        }
    }

    fn send(&mut self, msg: Message) -> Result<(), SenderError> {
        msg.send(self.writer.by_ref())?;
        Ok(())
    }

    fn handle(&mut self, cmd: Command) -> Result<(), SenderError> {
        match cmd {
            Command::Ping => {}
            Command::Shutdown => return Err(SenderError::Channel),
            Command::ClientHave(index) => self.handle_client_have(index)?,
            Command::PeerHave(index) => self.handle_peer_have(index)?,
            Command::BitFieldReceived => self.handle_bitfield()?,
            Command::Choke(b) => self.handle_client_choke(b)?,
            Command::PeerChoke(b) => self.handle_peer_choke(b)?,
            Command::SendChunk(index, begin, length) => {
                self.handle_send_chunk(index, begin, length)?
            }
        }
        Ok(())
    }

    fn handle_commands(&mut self) -> Result<(), SenderError> {
        loop {
            match self.rx.try_recv() {
                Ok(cmd) => self.handle(cmd)?,
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(_) => return Err(SenderError::Channel),
            }
        }
    }

    fn interested(&mut self, interested: bool) -> Result<(), SenderError> {
        {
            let mut s = self.state.write().unwrap();
            s.client_interested = interested;
        };
        if interested {
            self.send(Message::Interested)?;
            self.queue_pieces()?;
        } else {
            self.send(Message::NotInterested)?;
        }
        Ok(())
    }

    fn handle_client_have(&mut self, index: u32) -> Result<(), SenderError> {
        let avail = { self.availability.lock().unwrap()[index as usize] };
        if !avail {
            // Peer doesn't have piece
            self.send(Message::Have(index))?;
        }
        self.pending.remove(&index);
        Ok(())
    }

    fn handle_peer_have(&mut self, index: u32) -> Result<(), SenderError> {
        let needed = { self.store.read().unwrap().check_if_needed(index) };
        if needed {
            self.interested(true)?;
        }
        Ok(())
    }

    fn handle_bitfield(&mut self) -> Result<(), SenderError> {
        let mut needed = !self.store.read().unwrap().as_bitvec(true);
        needed &= self.availability.lock().unwrap().iter();
        if needed.iter().filter(|b| *b).take(1).next().is_some() {
            self.interested(true)?;
        }
        Ok(())
    }

    fn handle_client_choke(&mut self, choke: bool) -> Result<(), SenderError> {
        {
            let mut s = self.state.write().unwrap();
            s.client_choked = choke;
            info!("Peer {}: {:?}", self.peer_id, *s);
        }
        if choke {
            self.pieces.clear();
            self.send(Message::Choke)?;
        } else {
            self.send(Message::Unchoke)?;
        }
        Ok(())
    }

    fn handle_peer_choke(&mut self, choke: bool) -> Result<(), SenderError> {
        // On Choke, drop all queued and pending requests
        if choke {
            self.pending.clear();
            self.requests.clear();
            self.store
                .write()
                .unwrap()
                .clear_requests(self.peer_id.as_str());
        } else {
            self.queue_pieces()?;
        }
        Ok(())
    }

    fn handle_send_chunk(
        &mut self,
        index: u32,
        begin: u32,
        length: u32,
    ) -> Result<(), SenderError> {
        if index >= self.metainfo.num_pieces()
            || begin + length > self.metainfo.get_piece_size(index)
        {
            return Err(SenderError::InvalidRequest);
        }
        let piece = match self.store.read().unwrap().get(index) {
            Some(v) => v,
            None => return Err(SenderError::InvalidRequest),
        };
        self.pieces
            .push_back(Piece::new(index, begin, length, piece));
        Ok(())
    }

    pub fn queue_pieces(&mut self) -> Result<(), SenderError> {
        let state = self.state.read().unwrap().clone();
        if state.client_interested && !state.peer_choked && self.pending.len() <= QUEUE_LENGTH / 2 {
            debug!("Requesting {} pieces", QUEUE_LENGTH - self.pending.len());
            let res = self.store.write().unwrap().request_pieces(
                self.peer_id.as_str(),
                self.availability.lock().unwrap().clone(),
                (QUEUE_LENGTH - self.pending.len()) as u32,
            );
            match res {
                Ok(v) => {
                    for element in v.iter() {
                        self.pending.insert(*element);
                        self.requests.push_back(Message::Request(
                            *element,
                            0,
                            self.metainfo.get_piece_size(*element),
                        ))
                    }
                }
                _ => {
                    if self.pending.len() == 0 {
                        self.interested(false)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn start(mut self) {
        match self._start() {
            Err(e) => warn!("{}: {}", self.peer_id, e),
            _ => unreachable!(),
        }
        {
            self.store
                .write()
                .unwrap()
                .clear_requests(self.peer_id.as_str());
        }

        // Attempt to close TCP connection
        match self.writer.into_inner() {
            Err(e) => error!("{}: {}", self.peer_id, e),
            Ok(s) => match s.shutdown(Shutdown::Both) {
                Ok(_) => {}
                Err(e) => error!("{}: {}", self.peer_id, e),
            },
        }
    }
}
