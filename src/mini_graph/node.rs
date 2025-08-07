use heapless::spsc::{Consumer};

use crate::mini_graph::buffer::Frame;
use crate::mini_graph::bang::Bang;
use crate::nodes::audio::delay::DelayLine;


/// These different AudioUnits all have different strategies for fetching audio.
/// 
/// AudioNodes are chained in a DAG, and use their dependencies to calculate audio
/// 
/// DelayTap/Write nodes read/write to specific delay buffers, ignoring their input/outputs respectively
/// 
/// IONodes take a channel, maybe generic in the future
pub enum AudioUnit<const N: usize, const C: usize> {
    AudioNode { node: BoxedAudioNode<N, C> },
    DelayTapNode { node: BoxedDelayTapNode<N, C>},
    DelayWriteNode { node: BoxedDelayWriteNode<N, C>},
    IONode { node: BoxedIONode<N, C> }
}

pub trait AudioNode<const N: usize, const C: usize> {
    fn process(&mut self, inputs: &[Frame<N, C>], output: &mut Frame<N, C>){}
    fn handle_bang(&mut self, inputs: &[Bang], output: &mut Bang) {}
}

pub type BoxedAudioNode<const N: usize, const C: usize> = Box<dyn AudioNode<N, C> + Send> ;

pub trait DelayTapNode<const N: usize, const C: usize> {
    // Functions for registering the delay line with the AudioContext
    fn get_delay_line_index(&self) -> &usize;

    fn process(&mut self, delay_line: &DelayLine<N, C>, output: &mut Frame<N, C> ){}
    fn handle_bang(&mut self, inputs: &[Bang], output: &mut Bang) { }
}

pub type BoxedDelayTapNode<const N: usize, const C: usize> = Box<dyn DelayTapNode<N, C> + Send> ;

pub trait DelayWriteNode<const N: usize, const C: usize> {
    // Functions for registering the delay line with the AudioContext
    fn get_delay_line_index(&self) -> &usize;

    fn process(&mut self, inputs: &[Frame<N, C>], delay_line: &mut DelayLine<N, C>){}
    fn handle_bang(&mut self, inputs: &[Bang], output: &mut Bang) {}
}

pub type BoxedDelayWriteNode<const N: usize, const C: usize> = Box<dyn DelayWriteNode<N, C> + Send> ;

const MAXIMUM_QUEUE_SIZE: usize = 16;

pub trait IONode<const N: usize, const C: usize> {
    fn process(&mut self, output: &mut Frame<N, C>){}
    fn handle_bang(&mut self, inputs: &[Bang], output: &mut Bang) {}
    fn set_receiver(&mut self, receiver: Consumer<Frame<N, C>, MAXIMUM_QUEUE_SIZE>) {} // Take ownership of some receiver
}

pub type BoxedIONode<const N: usize, const C: usize> = Box<dyn IONode<N, C> + Send>;