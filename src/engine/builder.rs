use std::{cell::UnsafeCell, sync::Arc};

use arc_swap::ArcSwapOption;
use generic_array::GenericArray;

use crate::{
    engine::{graph::NodeKey, node::Node, runtime::Runtime},
    nodes::audio::{
        audio_ops::{ApplyOpMono, ApplyOpStereo}, delay::{DelayLine, DelayReadMono, DelayReadStereo}, osc::{OscMono, OscStereo}, sampler::{SamplerMono, SamplerStereo}, stereo::Stereo
    },
};

use typenum::{U1, U2};

// TODO: Port over proc macro from other repo
pub enum Nodes<const AF: usize> {
    // Osc
    OscMono,
    OscStereo,
    // Fan mono to stereo
    Stereo,
    // Sampler utils
    SamplerMono { props: Arc<ArcSwapOption<GenericArray<Vec<f32>, U1>>> },
    SamplerStereo { props: Arc<ArcSwapOption<GenericArray<Vec<f32>, U2>>> },
    // Delays
    DelayWriteMono { props: Arc<UnsafeCell<DelayLine<AF, U1>>> },
    DelayWriteStereo { props: Arc<UnsafeCell<DelayLine<AF, U2>>>},
    DelayReadMono { props: Arc<UnsafeCell<DelayLine<AF, U1>>> },
    DelayReadStereo { props: Arc<UnsafeCell<DelayLine<AF, U2>>>},
    // Ops
    AddMono { props: f32 },
    AddStereo { props: f32 },
    MultMono { props: f32 },
    MultStereo { props: f32 },
    // SvfMono,
    // SvfStereo
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum BuilderError {
    InvalidProps,
}

pub trait RuntimeBuilder<const AF: usize> {
    fn add_node_api(
        &mut self,
        node: Nodes<AF>,
    ) -> Result<NodeKey, BuilderError>;
}

impl<const AF: usize, const CF: usize, const C: usize> RuntimeBuilder<AF> for Runtime<AF, CF, C> {
    fn add_node_api(
        &mut self,
        node: Nodes<AF>,
    ) -> Result<NodeKey, BuilderError> {
        let node_created: Result<Box<dyn Node<AF, CF> + Send + 'static>, BuilderError> = match node
        {
            Nodes::OscMono => Ok(Box::new(OscMono::default())),
            Nodes::OscStereo => Ok(Box::new(OscStereo::default())),
            Nodes::Stereo => Ok(Box::new(Stereo::default())),
            // Samplers
            Nodes::SamplerMono { props } => Ok(Box::new(SamplerMono::new(props))),
            Nodes::SamplerStereo { props } => Ok(Box::new(SamplerStereo::new(props))),
            // Delay
            Nodes::DelayReadMono {props }=> Ok(Box::new(DelayReadMono::new(props))),
            Nodes::DelayReadStereo { props } => Ok(Box::new(DelayReadStereo::new(props))),
            Nodes::DelayWriteMono  {props }=> Ok(Box::new(DelayReadMono::new(props))),
            Nodes::DelayWriteStereo { props } => Ok(Box::new(DelayReadStereo::new(props))),
            // Ops
            Nodes::AddMono { props } => Ok(Box::new(ApplyOpMono::new(|a,b| a + b, props))),
            Nodes::AddStereo { props } => Ok(Box::new(ApplyOpStereo::new(|a,b| a + b, props))),
            Nodes::MultMono { props } => Ok(Box::new(ApplyOpMono::new(|a,b| a * b, props))),
            Nodes::MultStereo { props } => Ok(Box::new(ApplyOpStereo::new(|a,b| a * b, props))),
        };
        match node_created {
            Ok(node) => Ok(self.add_node(node)),
            Err(err) => Err(err),
        }
    }
}
