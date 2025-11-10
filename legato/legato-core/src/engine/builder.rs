use std::{ops::Mul, sync::Arc, time::Duration};

use arc_swap::ArcSwapOption;
use generic_array::{ArrayLength, GenericArray};

use crate::{
    engine::{
        audio_context::DelayLineKey,
        graph::NodeKey,
        node::{FrameSize, Node},
        runtime::{Runtime, RuntimeErased},
    },
    nodes::audio::{
        audio_ops::{ApplyOpMono, ApplyOpStereo},
        delay::{DelayLine, DelayReadMono, DelayReadStereo, DelayWriteMono, DelayWriteStereo},
        filters::fir::{FirMono, FirStereo},
        mixer::*,
        sampler::{SamplerMono, SamplerStereo},
        sine::{SineMono, SineStereo},
        stereo::Stereo,
        subgraph::Oversample2X,
        sweep::Sweep,
    },
};

use typenum::{Prod, U1, U2};

// TODO: Find nicer solution for arbitrary port size

pub enum Nodes<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    // Osc
    OscMono {
        freq: f32,
    },
    OscStereo {
        freq: f32,
    },
    // Fan mono to stereo
    Stereo,
    // Sampler utils
    SamplerMono {
        props: Arc<ArcSwapOption<GenericArray<Vec<f32>, U1>>>,
    },
    SamplerStereo {
        props: Arc<ArcSwapOption<GenericArray<Vec<f32>, U2>>>,
    },
    // Delays
    DelayWriteMono {
        props: Duration,
    },
    DelayWriteStereo {
        props: Duration,
    },
    DelayReadMono {
        key: DelayLineKey,
        offsets: [Duration; 1],
    },
    DelayReadStereo {
        key: DelayLineKey,
        offsets: [Duration; 2],
    },
    // Filter
    FirMono {
        kernel: Vec<f32>,
    },
    FirStereo {
        kernel: Vec<f32>,
    },
    // Ops
    AddMono {
        props: f32,
    },
    AddStereo {
        props: f32,
    },
    MultMono {
        props: f32,
    },
    MultStereo {
        props: f32,
    },
    // Mixers
    StereoMixer,           // U2 -> U2
    StereoToMono,          // U2 -> U1
    TwoTrackStereoMixer,   // U4 -> U2
    FourTrackStereoMixer,  // U8 -> U2
    EightTrackStereoMixer, // U16 -> U2
    FourToMonoMixer,       // U8  -> U1
    TwoTrackMonoMixer,     // U4 -> U1
    // SvfMono,
    // SvfStereo
    // Subgraph
    Subgraph {
        runtime: Box<dyn RuntimeErased<AF, CF> + Send + 'static>,
    },
    Subgraph2XOversampled {
        runtime: Box<dyn RuntimeErased<Prod<AF, U2>, CF> + Send + 'static>,
    },
    // Utils
    Sweep {
        range: (f32, f32),
        duration: Duration,
    },
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum BuilderError {
    InvalidProps,
}

/// Sometime, certain information can only be passed out from the runtime builder.
///
/// For instance, adding a delay_write requires a slotmap key that is only now constructed.
pub enum AddNodeResponse {
    DelayWrite(DelayLineKey),
}

pub trait RuntimeBuilder<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    fn add_node_api(
        &mut self,
        node: Nodes<AF, CF>,
    ) -> Result<(NodeKey, Option<AddNodeResponse>), BuilderError>;
}

impl<AF, CF, C, Ci> RuntimeBuilder<AF, CF> for Runtime<AF, CF, C, Ci>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    Ci: ArrayLength,
    C: ArrayLength,
{
    fn add_node_api(
        &mut self,
        node: Nodes<AF, CF>,
    ) -> Result<(NodeKey, Option<AddNodeResponse>), BuilderError> {
        let node_created: (
            Result<Box<dyn Node<AF, CF> + Send + 'static>, BuilderError>,
            Option<AddNodeResponse>,
        ) = match node {
            Nodes::OscMono { freq } => (Ok(Box::new(SineMono::new(freq, 0.0))), None),
            Nodes::OscStereo { freq } => (Ok(Box::new(SineStereo::new(freq, 0.0))), None),
            Nodes::Stereo => (Ok(Box::new(Stereo::default())), None),
            // Samplers
            Nodes::SamplerMono { props } => (Ok(Box::new(SamplerMono::new(props))), None),
            Nodes::SamplerStereo { props } => (Ok(Box::new(SamplerStereo::new(props))), None),
            // Delay reads
            Nodes::DelayReadMono { key, offsets } => (
                Ok(Box::new(DelayReadMono::new(
                    key,
                    *GenericArray::from_slice(&offsets),
                ))),
                None,
            ),
            Nodes::DelayReadStereo { key, offsets } => (
                Ok(Box::new(DelayReadStereo::new(
                    key,
                    *GenericArray::from_slice(&offsets),
                ))),
                None,
            ),
            // Delay writes (keep as-is)
            Nodes::DelayWriteMono { props } => {
                let ctx = self.get_context_mut();
                let samples = ctx.get_sample_rate();
                let delay_capacity = props.as_secs_f32() * samples;

                let delay_line_mono = DelayLine::<AF, U1>::new(delay_capacity as usize);

                let key = ctx.add_delay_line(Box::new(delay_line_mono));

                (
                    Ok(Box::new(DelayWriteMono::new(key))),
                    Some(AddNodeResponse::DelayWrite(key)),
                )
            }
            Nodes::DelayWriteStereo { props } => {
                let ctx = self.get_context_mut();
                let samples = ctx.get_sample_rate();
                let delay_capacity = props.as_secs_f32() * samples;

                let delay_line_stereo = DelayLine::<AF, U2>::new(delay_capacity as usize);
                let key = ctx.add_delay_line(Box::new(delay_line_stereo));

                (
                    Ok(Box::new(DelayWriteStereo::new(key))),
                    Some(AddNodeResponse::DelayWrite(key)),
                )
            }
            // Ops
            Nodes::AddMono { props } => (Ok(Box::new(ApplyOpMono::new(|a, b| a + b, props))), None),
            Nodes::AddStereo { props } => {
                (Ok(Box::new(ApplyOpStereo::new(|a, b| a + b, props))), None)
            }
            Nodes::MultMono { props } => {
                (Ok(Box::new(ApplyOpMono::new(|a, b| a * b, props))), None)
            }
            Nodes::MultStereo { props } => {
                (Ok(Box::new(ApplyOpStereo::new(|a, b| a * b, props))), None)
            }
            // Filters
            Nodes::FirMono { kernel } => (Ok(Box::new(FirMono::new(kernel))), None),
            Nodes::FirStereo { kernel } => (Ok(Box::new(FirStereo::new(kernel))), None),
            // Mixers
            Nodes::StereoMixer => (Ok(Box::new(StereoMixer::default())), None),
            Nodes::StereoToMono => (Ok(Box::new(StereoToMonoMixer::default())), None),
            Nodes::FourToMonoMixer => (Ok(Box::new(FourToMonoMixer::default())), None),
            Nodes::TwoTrackStereoMixer => (Ok(Box::new(TwoTrackStereoMixer::default())), None),
            Nodes::FourTrackStereoMixer => (Ok(Box::new(FourTrackStereoMixer::default())), None),
            Nodes::EightTrackStereoMixer => (Ok(Box::new(EightTrackStereoMixer::default())), None),
            Nodes::TwoTrackMonoMixer => (Ok(Box::new(TwoTrackMonoMixer::default())), None),
            Nodes::Subgraph { runtime } => (Ok(runtime), None),
            Nodes::Subgraph2XOversampled { runtime } => {
                (Ok(Box::new(Oversample2X::<AF, CF, C>::new(runtime))), None)
            }

            // Utils
            Nodes::Sweep { range, duration } => (Ok(Box::new(Sweep::new(range, duration))), None),
        };

        match node_created {
            (Ok(node), maybe_response) => Ok((self.add_node(node), maybe_response)),
            (Err(err), _) => Err(err),
        }
    }
}
