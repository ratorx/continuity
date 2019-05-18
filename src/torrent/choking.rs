use crate::connection::Connection;
use log::{self, debug, error, info, warn};
use rand::distributions::{Distribution, Uniform};
use std::cmp::Reverse;
use std::collections::HashSet;

pub struct Choke {
    connections: Vec<Connection>,
    optimistic_unchoke: Option<Connection>,
}

impl Choke {
    pub fn new() -> Self {
        Self {
            connections: Vec::new(),
            optimistic_unchoke: None,
        }
    }

    pub fn add(&mut self, conn: Connection) {
        self.connections.push(conn);
    }

    fn pick_optimistic_unchoke(&mut self) -> Option<Connection> {
        if self.connections.len() == 0 {
            return None;
        }

        let unchoke = self
            .connections
            .swap_remove(Uniform::from(0..self.connections.len()).sample(&mut rand::thread_rng()));
        Some(unchoke)
    }

    pub fn setup(&mut self, optimistic_unchoke: bool) {
        // Get rid of duplicate connections
        // After this point, assume any connection will stay valid until next time this loop
        // is run - i.e. ignore the errors when they aren't
        self.connections.retain(|c| !c.is_shutdown());

        // Early optimistic unchoke
        if self.optimistic_unchoke.is_none()
            || self.optimistic_unchoke.as_ref().unwrap().is_shutdown()
        {
            let c = self.pick_optimistic_unchoke();
            self.optimistic_unchoke = c;
        // Scheduled optimistic unchoke
        } else if optimistic_unchoke {
            let c = self.optimistic_unchoke.take().unwrap();
            self.connections.push(c);
            self.optimistic_unchoke = self.pick_optimistic_unchoke();
        }

        // Update the snapshots (including optimistic unchoke)
        self.connections
            .iter_mut()
            .for_each(|c| c.update_snapshot());
        if self.optimistic_unchoke.is_some() {
            self.optimistic_unchoke.as_mut().unwrap().update_snapshot();
        }
    }

    pub fn download(&mut self, optimistic_unchoke: bool) {
        self.setup(optimistic_unchoke);

        // Determine downloaders
        self.connections
            .sort_by_key(|c| Reverse(c.snapshot.downloaded));
        // Determine the set of peers currently downloading from client
        let downloaders: HashSet<_> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.snapshot.state.peer_choked && c.snapshot.state.peer_interested)
            .take(4)
            .map(|(i, _)| i)
            .collect();
        // Determine the minimum threshold for downloading from client
        let downloader_threshold: u64 = downloaders
            .iter()
            .cloned()
            .map(|i| self.connections[i].snapshot.downloaded)
            .min()
            .unwrap_or(0);
        // Determine peers to unchoke
        let unchoked: HashSet<_> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                !c.snapshot.state.peer_choked
                    && !c.snapshot.state.peer_interested
                    && (c.snapshot.downloaded > downloader_threshold || downloader_threshold == 0)
            })
            .map(|(i, _)| i)
            .collect();
        debug!("Connections: {:?}", self.connections);
        debug!(
            "Unchoked: {:?}",
            &unchoked.iter().map(|i| &self.connections[*i])
        );
        debug!(
            "Downloaders: {:?}",
            &downloaders.iter().map(|i| &self.connections[*i])
        );
        for i in 0..self.connections.len() {
            let conn = &self.connections[i];
            // Laid out in weird way to avoid multiple calls for set inclusion
            // Match anything that should be unchoked
            if downloaders.contains(&i) || unchoked.contains(&i) {
                // Unchoke it if it is choked
                if conn.snapshot.state.client_choked {
                    let _ = conn.choke(false);
                }
            // Matches anything that shouldnt be choked, but is
            } else if !conn.snapshot.state.client_choked {
                let _ = conn.choke(true);
            }
        }

        match &self.optimistic_unchoke {
            Some(c) if c.snapshot.state.client_choked => {
                let _ = c.choke(false);
            }
            _ => {}
        }
    }

    pub fn upload(&mut self, optimistic_unchoke: bool) {
        self.setup(optimistic_unchoke);

        // Determine uploaders
        self.connections
            .sort_by_key(|c| Reverse(c.snapshot.uploaded));
        let uploaders: HashSet<_> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| c.snapshot.state.peer_interested)
            .take(4)
            .map(|(i, _)| i)
            .collect();
        let uploader_threshold: u64 = uploaders
            .iter()
            .cloned()
            .map(|i| self.connections[i].snapshot.uploaded)
            .min()
            .unwrap_or(0);
        // Determine peers to unchoke
        let unchoked: HashSet<_> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                !c.snapshot.state.peer_interested
                    && (c.snapshot.uploaded > uploader_threshold || uploader_threshold == 0)
            })
            .map(|(i, _)| i)
            .collect();
        debug!("Connections: {:?}", self.connections);
        debug!(
            "Unchoked: {:?}",
            &unchoked.iter().map(|i| &self.connections[*i])
        );
        debug!(
            "Uploaders: {:?}",
            &uploaders.iter().map(|i| &self.connections[*i])
        );
        for i in 0..self.connections.len() {
            let conn = &self.connections[i];
            // Laid out in weird way to avoid multiple calls for set inclusion
            // Match anything that should be unchoked
            if uploaders.contains(&i) || unchoked.contains(&i) {
                // Unchoke it if it is choked
                if conn.snapshot.state.client_choked {
                    let _ = conn.choke(false);
                }
            // Matches anything that shouldnt be choked, but is
            } else if !conn.snapshot.state.client_choked {
                let _ = conn.choke(true);
            }
        }

        match &self.optimistic_unchoke {
            Some(c) if c.snapshot.state.client_choked => {
                let _ = c.choke(false);
            }
            _ => {}
        }
    }
}
