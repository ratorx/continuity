mod receiver;
mod sender;

use crate::metainfo::Metainfo;
use crate::storage::PieceStore;
use bitvec::{bitvec, BitVec};
use receiver::Receiver;
use sender::Sender;
use std::collections::HashSet;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, BufReader, BufWriter};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;

#[derive(Clone)]
pub struct State {
    pub client_choked: bool,
    pub client_interested: bool,
    pub peer_choked: bool,
    pub peer_interested: bool,
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "client(c: {}, i: {}) peer(c:{}, i:{})",
            self.client_choked, self.client_interested, self.peer_choked, self.peer_interested
        )
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            client_choked: true,
            client_interested: false,
            peer_choked: true,
            peer_interested: false,
        }
    }
}

#[derive(Debug)]
struct Metrics {
    pub downloaded: Arc<Mutex<u64>>,
    pub uploaded: Arc<Mutex<u64>>,
}

#[derive(Default, Debug)]
pub struct Snapshot {
    pub downloaded: u64,
    pub uploaded: u64,
    pub availability: BitVec,
    pub state: State,
}

#[derive(Debug)]
pub enum Command {
    // Test whether channel is open
    Ping,
    // Triggered by client only when download finished
    // May be triggered at any time by received
    Shutdown,
    // Triggered by Piece Store
    ClientHave(u32),
    // Triggered by receiver
    PeerHave(u32),
    // Initial have
    BitFieldReceived,
    // Triggered by Choke
    Choke(bool),
    // Triggered by receiver
    PeerChoke(bool),
    // Triggered by receiver when piece requested
    SendChunk(u32, u32, u32),
}

pub struct ConnInfo {
    pub store: Arc<RwLock<PieceStore>>,
    pub metainfo: Arc<Metainfo>,
    pub reader_buffer_len: Option<usize>,
    pub writer_buffer_len: Option<usize>,
    pub id: Arc<String>,
    pub client_id: Arc<String>,
}

pub struct Connection {
    pub tx: mpsc::Sender<Command>,
    receiver_handle: thread::JoinHandle<()>,
    sender_handle: thread::JoinHandle<()>,
    availability: Arc<Mutex<BitVec>>,
    pub state: Arc<RwLock<State>>,
    metrics: Metrics,
    pub snapshot: Snapshot,
    pub id: Arc<String>,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Connection({})", self.id)
    }
}

impl Connection {
    pub fn connect<A: ToSocketAddrs>(addr: A, ci: ConnInfo) -> Result<Self, io::Error> {
        let stream = TcpStream::connect(addr)?;
        Connection::new(stream, ci)
    }

    pub fn new(stream: TcpStream, ci: ConnInfo) -> Result<Self, io::Error> {
        let (tx, rx) = mpsc::channel();
        let reader = match ci.reader_buffer_len {
            None => BufReader::new(stream.try_clone()?),
            Some(x) => BufReader::with_capacity(x, stream.try_clone()?),
        };

        let writer = match ci.writer_buffer_len {
            None => BufWriter::new(stream.try_clone()?),
            Some(x) => BufWriter::with_capacity(x, stream.try_clone()?),
        };

        let state = Arc::new(RwLock::new(State::default()));
        let availability = Arc::new(Mutex::new(bitvec![0; ci.metainfo.num_pieces() as usize]));

        let receiver = Receiver {
            tx: tx.clone(),
            piece_buffer: HashMap::new(),
            state: state.clone(),
            store: ci.store.clone(),
            speed: Mutex::new(None),
            availability: availability.clone(),
            reader,
            peer_id: ci.id.clone(),
            client_id: ci.client_id.clone(),
            metainfo: ci.metainfo.clone(),
            bitfield_received: false,
            num_downloaded: Arc::new(Mutex::new(0)),
        };

        let sender = Sender {
            rx,
            requests: VecDeque::new(),
            pending: HashSet::new(),
            pieces: VecDeque::new(),
            state: state.clone(),
            store: ci.store.clone(),
            availability: availability.clone(),
            metainfo: ci.metainfo.clone(),
            peer_id: ci.id.clone(),
            client_id: ci.client_id.clone(),
            writer,
            num_uploaded: Arc::new(Mutex::new(0)),
        };

        let metrics = Metrics {
            downloaded: receiver.num_downloaded.clone(),
            uploaded: sender.num_uploaded.clone(),
        };

        let receiver_handle = thread::spawn(move || receiver.start());
        let sender_handle = thread::spawn(move || sender.start());

        // Register with store
        ci.store.read().unwrap().register(tx.clone());

        Ok(Connection {
            tx,
            receiver_handle,
            sender_handle,
            availability: availability.clone(),
            state: state.clone(),
            metrics,
            snapshot: Default::default(),
            id: ci.id,
        })
    }

    pub fn update_snapshot(&mut self) {
        self.snapshot.availability = { self.availability.lock().unwrap().clone() };
        self.snapshot.state = { self.state.read().unwrap().clone() };
        self.snapshot.downloaded = {
            let mut x = self.metrics.downloaded.lock().unwrap();
            let y = *x;
            *x = 0;
            y
        };
        self.snapshot.uploaded = {
            let mut x = self.metrics.uploaded.lock().unwrap();
            let y = *x;
            *x = 0;
            y
        };
    }

    pub fn is_shutdown(&self) -> bool {
        if let Err(_) = self.tx.send(Command::Ping) {
            return true;
        }
        false
    }

    pub fn choke(&self, choke: bool) -> Result<(), mpsc::SendError<Command>> {
        self.tx.send(Command::Choke(choke))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}
