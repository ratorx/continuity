use crate::connection::Command;
use crate::metainfo::Metainfo;
use crate::selection::{Selector, State};
use bitvec::BitVec;
use log::{self, debug, error, info, warn};
use std::collections::{HashMap, HashSet};
use std::default::Default;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time;

pub enum PieceStatus {
    Requested(String),
    Downloaded(Arc<Vec<u8>>),
}

pub struct PieceStore {
    data: Vec<Option<PieceStatus>>,
    inprogress: HashMap<String, HashSet<u32>>, // Used to deal with choke requests efficiently
    pub left: u32,                             // Used to deal with completion checks efficiently
    handlers: Mutex<Vec<mpsc::Sender<Command>>>,
    next: usize,
    selector: Box<dyn Selector + Send + Sync>,
    start: time::Instant,
}

impl PieceStore {
    pub fn new(mi: &Metainfo, s: Box<dyn Selector + Send + Sync>) -> Self {
        let mut data = Vec::with_capacity(mi.num_pieces() as usize);
        data.resize_with(mi.num_pieces() as usize, Default::default);
        PieceStore {
            left: data.len() as u32,
            data,
            inprogress: HashMap::new(),
            handlers: Mutex::new(Vec::new()),
            next: 0,
            selector: s,
            start: time::Instant::now(),
        }
    }

    pub fn register(&self, tx: mpsc::Sender<Command>) {
        self.handlers.lock().unwrap().push(tx)
    }

    pub fn as_bitvec(&self, include_requested: bool) -> BitVec {
        self.data
            .iter()
            .map(|v| match v {
                None => false,
                Some(PieceStatus::Requested(_)) if !include_requested => false,
                Some(_) => true,
            })
            .collect()
    }

    pub fn get(&self, index: u32) -> Option<Arc<Vec<u8>>> {
        match &self.data[index as usize] {
            Some(PieceStatus::Downloaded(v)) => Some(v.clone()),
            _ => None,
        }
    }

    pub fn bootstrap<P: AsRef<Path>>(&mut self, metainfo: &Metainfo, path: P) -> io::Result<()> {
        let mut f = File::open(path)?;
        for (i, e) in self.data.iter_mut().enumerate() {
            let mut v = vec![0; metainfo.get_piece_size(i as u32) as usize];
            f.read_exact(&mut v)?;
            *e = Some(PieceStatus::Downloaded(Arc::new(v)));
        }
        self.left = 0;
        self.next = self.data.len();
        Ok(())
    }

    pub fn check_if_needed(&self, index: u32) -> bool {
        match &self.data[index as usize] {
            Some(_) => false,
            None => true,
        }
    }

    pub fn store(&mut self, id: &str, index: u32, piece: Arc<Vec<u8>>) {
        self.data[index as usize] = Some(PieceStatus::Downloaded(piece));
        match self.inprogress.get_mut(id) {
            Some(hs) => {
                hs.remove(&index);
            }
            None => {}
        };
        self.left -= 1;
        // Inform connections that new piece received and get rid of closed connections
        self.handlers
            .lock()
            .unwrap()
            .retain(|t| t.send(Command::ClientHave(index)).is_ok());
        info!("Datapoint {} {}", index, self.start.elapsed().as_millis());
        self.write_to_stdout();
    }

    fn write_to_stdout(&mut self) {
        if self.next >= self.data.len() {
            return;
        }
        let mut stdout = io::stdout();
        while let Some(Some(PieceStatus::Downloaded(v))) = &self.data.get(self.next) {
            stdout.write(&v).unwrap();
            self.next += 1;
        }
    }

    fn mark(&mut self, id: &str, index: u32) {
        match self.inprogress.get_mut(id) {
            Some(x) => {
                x.insert(index);
            }
            None => {
                let mut hs = HashSet::new();
                hs.insert(index);
                self.inprogress.insert(id.to_owned(), hs);
            }
        }
        self.data[index as usize] = Some(PieceStatus::Requested(id.to_owned()));
    }

    pub fn clear_requests(&mut self, id: &str) {
        if let Some(hs) = self.inprogress.remove(id) {
            for index in hs.into_iter() {
                match self.data[index as usize] {
                    Some(PieceStatus::Requested(ref x)) if x.as_str() == id => {
                        self.data[index as usize] = None
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn request_pieces(
        &mut self,
        id: &str,
        mut availability: BitVec,
        n: u32,
    ) -> Result<Vec<u32>, ()> {
        availability |= self.as_bitvec(false);
        let v = self.selector.request_pieces(
            id,
            State {
                required: !self.as_bitvec(true),
                available: availability,
            },
            n,
        );

        if v.len() == 0 {
            return Err(());
        }

        v.iter().for_each(|i| self.mark(id, *i));

        Ok(v)
    }
}
