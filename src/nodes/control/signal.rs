use std::sync::Arc;
use portable_atomic::{AtomicF32, Ordering::{Acquire, Release}};

use crate::mini_graph::{buffer::Frame, node::AudioNode};

pub enum ParamError {
    InvalidRange
}

#[derive(Clone)]
pub struct Param {
    min: f32,
    max: f32,
    val: Arc<AtomicF32>
}

impl Param {
    pub fn new(initial: Arc<AtomicF32>, min: f32, max: f32) -> Self {
        Self {
            val: initial,
            min,
            max
        }
    }
    pub fn set(&self, new_val: f32) -> Result<(), ParamError>{
        if new_val > self.max || new_val < self.min {
            return Err(ParamError::InvalidRange);
        }
        self.val.store(new_val, Release);
        return Ok(())
    }
    pub fn get(&self) -> f32 {
        self.val.load(Acquire)
    }
}

pub struct Signal {
    val: Arc<AtomicF32>
}
impl Signal {
    pub fn from(initial: Arc<AtomicF32>) -> Self {
        Self { val: initial }
    }
}
impl<const N: usize, const C: usize> AudioNode<N, C> for Signal {
    fn process(&mut self, _: &[Frame<N, C>], output: &mut Frame<N, C>) {
        let value = self.val.load(Acquire);
        for n in 0..N {
            for c in 0..C {
                output[c][n] = value;
            }
        }
    }
}

pub fn build_signal(min: f32, max: f32, initial: f32) -> (Signal, Param){
    let val = Arc::new(AtomicF32::new(initial));
    let param = Param::new(val.clone(), min, max);
    let signal_node = Signal::from(val);

    (signal_node, param)
}