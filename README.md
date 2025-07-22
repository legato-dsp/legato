# mini-graph

This project is aimed around creating a minimal audio graph in Rust, useful for experimentation or prototyping a larger audio application. I wanted a more lightweight DX than FunDSP, but FunDSP is far more feature complete. For the time being, I would suggest using that for any real audio applications.

### Example Node

You can define a basic gain node like so:

```rust
use crate::node::Node;
use crate::buffer::Frame;

pub struct Gain<const FRAME_SIZE: usize> {
    gain: f32 // For nodes using input's from other nodes, keep it here, but you can also easily make this an Arc<Atomic> and share to manipulate elsewhere
}
impl<const N: usize> Gain<N> {
    pub fn new(gain: f32) -> Self {
        Self {
            gain
        }
    }
}
impl <const N: usize, const C: usize> Node<N, C> for Gain<N> {
    fn process(&mut self, inputs: &[Frame<N, C>], output: &mut Frame<N, C>){
        // This node only takes an input of one stereo buffer.
        let input = inputs[0];
        for n in 0..N { // For ever sample in our frame size
            for c in 0..C { // For ever channel in our frame
                output[c][n] = (input[c][n] * self.gain).clamp(-1.0 , 1.0);
            }
        }
    }
}
```

### Planned Features

- Using Graphs as audio nodes themselves, allowing to say create a large reverb graph and use that as its own node
- Audio/midi input nodes
- Proc macro for quickly generating graphs for prototyping
- Sized/fixed graph for no_std environments, final deployments, etc.
- Hopefully some sort of SIMD acceleration for more expensive operations like reverb, fft, etc.