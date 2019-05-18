use super::{Inorder, Rare, Selector, State};
use rand::prelude::*;
use smart_default::SmartDefault;

#[derive(SmartDefault)]
pub struct Bitos {
    inorder: Inorder,
    rare: Rare,
    #[default = 0.8]
    pub inorder_p: f64,
}

impl Selector for Bitos {
    fn request_pieces(&mut self, id: &str, state: State, n: u32) -> Vec<u32> {
        let mut rng = rand::thread_rng();
        let mut num_inorder = 0;
        let mut num_rare = 0;
        for _ in 0..n {
            let r: f64 = rng.gen();
            if r <= self.inorder_p {
                num_inorder += 1;
            } else {
                num_rare += 1;
            }
        }

        let mut state_c = state.clone();
        let mut v = self.inorder.request_pieces(id, state, num_inorder);
        v.iter()
            .for_each(|i| state_c.required.set(*i as usize, false));
        v.extend(self.rare.request_pieces(id, state_c, num_rare));
        v
    }
}
