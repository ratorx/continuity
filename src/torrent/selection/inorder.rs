use super::{Selector, State};

#[derive(Default)]
pub struct Inorder {}

impl Selector for Inorder {
    fn request_pieces(&mut self, _: &str, mut state: State, n: u32) -> Vec<u32> {
        state.available &= state.required;
        // let v: Vec<_> = availability
        //     .iter()
        //     .enumerate()
        //     .filter(|(_, b)| *b)
        //     .map(|(i, _)| i as u32)
        //     .take(n as usize)
        //     .collect();
        return state
            .available
            .iter()
            .enumerate()
            .filter(|(_, b)| *b)
            .map(|(i, _)| i as u32)
            .take(n as usize)
            .collect();
    }
}
