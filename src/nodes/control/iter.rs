use crate::mini_graph::{bang::Bang, node::Node};

pub struct BangIter<'a, const C: usize, const N: usize> {
    position: usize,
    values: &'a[Bang],
}
impl<'a, const C: usize, const N: usize> BangIter<'a, C, N> {
    pub fn new(values: &'a[Bang]) -> Self {
        Self {
            position: 0,
            values
        }
    }
}
impl<'a, const C: usize, const N: usize> Node<C, N> for BangIter<'a, C, N>{
    fn handle_bang(&mut self, inputs: &[Bang], output: &mut Bang) {
        for input in inputs {
            if *input == Bang::Bang {
                *output = self.values[self.position];
                if self.position < self.values.len() - 1 {
                    self.position += 1;
                }
                else {
                    self.position = 0;
                }
                return
            }
        }

    }
}