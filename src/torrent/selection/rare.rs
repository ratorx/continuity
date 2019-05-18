use super::{Selector, State};
use bitvec::BitVec;
use log::{self, debug, error, info, warn};
use rand::prelude::*;
use std::cmp::min;
use std::collections::HashMap;

#[derive(Default)]
pub struct Rare {
    pub history: HashMap<String, BitVec>,
    pub rarity: Vec<usize>,
}

impl Selector for Rare {
    fn request_pieces(&mut self, id: &str, state: State, n: u32) -> Vec<u32> {
        // Satisfy integrity of internal data
        let required = state.required;
        if self.history.contains_key(id) {
            let availability = self.history.get_mut(id).unwrap();
            // Set difference of historically available and currently available
            let mut bv = !state.available.clone();
            bv &= availability.iter();
            // Update history
            *availability = state.available;
            self.update_rarity(&bv);
        } else {
            self.update_rarity(&state.available);
            self.history.insert(id.to_owned(), state.available);
        }

        // Create a vector of piece indices, filtered by required and sorted by rarity
        let mut v: Vec<u32> = (0..required.len())
            .filter(|i| required[*i])
            .map(|i| i as u32)
            .collect();
        if v.len() == 0 {
            return v;
        }
        v.sort_by_key(|i| self.rarity[*i as usize]);

        // Retain all pieces with acceptable rarity
        let max_rarity = self.rarity[v[min(n as usize, v.len() - 1)] as usize];
        let mut ret: Vec<u32> = v
            .iter()
            .take_while(|i| self.rarity[**i as usize] < max_rarity)
            .map(|i| *i)
            .collect();
        let mut v = v.split_off(ret.len());
        let mut rng = rand::thread_rng();
        v.shuffle(&mut rng);
        v.truncate(n as usize - ret.len());
        debug!("Shuffled {:?}", v);
        ret.extend(v.into_iter());

        // Shrink vector to required size and return
        debug!("Piece Requests: {:?}", ret);
        ret
    }
}

impl Rare {
    fn update_rarity(&mut self, bv: &BitVec) {
        if self.rarity.len() == 0 {
            self.rarity = vec![0; bv.len()]
        }
        self.rarity
            .iter_mut()
            .zip(bv.iter())
            .for_each(|(rarity, update)| {
                if update {
                    *rarity += 1;
                }
            });
    }
}

// Compiler cannot implement sync automatically because BitVec is not sync
// This is a library issue; BitVec should be safe to synchronise
unsafe impl Sync for Rare {}
