use bitvec::BitVec;

pub mod bitos;
pub use bitos::Bitos;
pub mod inorder;
pub use inorder::Inorder;
pub mod rare;
pub use rare::Rare;

#[derive(Clone)]
pub struct State {
    pub required: BitVec,  // Vector of pieces client needs
    pub available: BitVec, // Vector of available pieces
}

pub trait Selector {
    fn request_pieces(&mut self, id: &str, state: State, n: u32) -> Vec<u32>;
}
