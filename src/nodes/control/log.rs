use crate::mini_graph::bang::Bang;
use crate::mini_graph::node::Node;

pub struct Log<const N: usize, const C: usize> {}
impl<const N: usize, const C: usize>  Node<N,C> for Log<N, C>{
    fn handle_bang(&mut self, inputs: &[Bang], _: &mut Bang) {
        if let Some(bang) = inputs.get(0){
            match bang {
                Bang::Empty => (),
                item => println!("{:?}", item)
            }
        }
    }
}