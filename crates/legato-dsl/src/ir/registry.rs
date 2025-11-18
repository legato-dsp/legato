use std::{
    collections::{BTreeSet, HashMap},
    ops::Mul,
    time::Duration,
};

use legato_core::engine::{builder::AddNode, node::FrameSize};
use typenum::{Prod, U2};

use crate::ir::{ValidationError, params::Params};

/// A node registry trait that let's users extend the graph logic
/// to make their own node namespaces. For example, you could make a
/// reverb namespace that has a bunch of primitives you might need,
/// or you could make a physics namespace with physics logic.
pub trait NodeRegistry<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    fn lower_to_ir(
        &self,
        name: &String,
        params: Option<&Params>,
    ) -> Result<AddNode<AF, CF>, ValidationError>;
}

/// The default container of node registries.
///
/// Users can make their own registries with their
/// own pairs. This means that when you're using
/// the graph, you can choose which namespaces
/// and nodes are in there, extend it on your own, etc.
///
/// To do this yourself, implement the NodeRegistry trait.
/// You can then extend the Legato registry container,
/// or make your own at a later time.
pub struct LegatoRegistryContainer<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    namespaces: HashMap<String, Box<dyn NodeRegistry<AF, CF>>>,
}

impl<AF, CF> LegatoRegistryContainer<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    pub fn new() -> Self {
        let mut namespaces = HashMap::new();
        namespaces.insert(
            String::from("audio"),
            Box::new(AudioRegistry::default()) as Box<dyn NodeRegistry<AF, CF>>,
        );
        Self { namespaces }
    }
    pub fn get(
        &self,
        namespace: &String,
        node: &String,
        params: Option<&Params>,
    ) -> Result<AddNode<AF, CF>, ValidationError> {
        if let Some(ns) = self.namespaces.get(namespace) {
            let add_node = ns.lower_to_ir(node, params)?;
            return Ok(add_node);
        }

        Err(ValidationError::NamespaceNotFound(format!(
            "Could not find namespace {}",
            namespace
        )))
    }
}

macro_rules! param_list {
    ($($param:expr),* $(,)?) => {
        {
            let mut set = BTreeSet::new();
            $(set.insert(String::from($param));)*
            set
        }
    };
}

/// One of the default registries, audio deals
/// with common audio effects. This may be renamed
/// in the future.
#[derive(Default)]
pub struct AudioRegistry;

impl<AF, CF> NodeRegistry<AF, CF> for AudioRegistry
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    fn lower_to_ir(
        &self,
        name: &String,
        params: Option<&Params>,
    ) -> Result<AddNode<AF, CF>, ValidationError> {
        // TODO: Not in love with this. Maybe a macro or reflection library?
        match name.as_str() {
            // Osc
            "sine_mono" => {
                if let Some(p) = params {
                    p.validate(&param_list!("freq")).unwrap();
                }
                let freq = params.and_then(|p| p.get_f32("freq")).unwrap_or(440.0);
                Ok(AddNode::SineMono { freq })
            }
            "sine_stereo" => {
                if let Some(p) = params {
                    p.validate(&param_list!("freq")).unwrap();
                }
                let freq = params.and_then(|p| p.get_f32("freq")).unwrap_or(440.0);
                Ok(AddNode::SineStereo { freq })
            }
            // Fan mono to stereo
            "stereo" => Ok(AddNode::Stereo),
            "sampler_mono" => {
                let p = params
                    .ok_or(ValidationError::MissingRequiredParameter(String::from(
                        "Sampler requires sample key",
                    )))
                    .unwrap();

                let p_list = param_list!("sample_name");

                p.validate(&p_list).unwrap();
                p.required(&p_list).unwrap();

                let sampler_name = p.get_str("sampler_name").unwrap();

                Ok(AddNode::SamplerMono {
                    sampler_name: sampler_name,
                })
            }
            "sampler_stereo" => {
                let p = params
                    .ok_or(ValidationError::MissingRequiredParameter(String::from(
                        "Sampler requires sample key",
                    )))
                    .unwrap();

                let p_list = param_list!("sample_name");

                p.validate(&p_list).unwrap();
                p.required(&p_list).unwrap();

                let sampler_name = p.get_str("sampler_name").unwrap();

                Ok(AddNode::SamplerStereo {
                    sampler_name: sampler_name,
                })
            }
            // Delays
            "delay_write_mono" => {
                let p = params
                    .ok_or(ValidationError::MissingRequiredParameter(String::from(
                        "Delay write mono requires delay key",
                    )))
                    .unwrap();

                let p_list = param_list!("delay_name", "delay_length");

                p.validate(&p_list).unwrap();
                p.required(&param_list!("delay_name")).unwrap();

                let delay_name = p.get_str("delay_name").unwrap();
                let delay_length = p
                    .get_duration("delay_length")
                    .unwrap_or(Duration::from_secs(1));

                Ok(AddNode::DelayWriteMono {
                    delay_name: delay_name,
                    delay_length: delay_length,
                })
            }
            "delay_write_stereo" => {
                let p = params
                    .ok_or(ValidationError::MissingRequiredParameter(String::from(
                        "Delay write stereo requires delay key",
                    )))
                    .unwrap();

                let p_list = param_list!("delay_name", "delay_length");

                p.validate(&p_list).unwrap();
                p.required(&param_list!("delay_name")).unwrap();

                let delay_name = p.get_str("delay_name").unwrap();
                let delay_length = p
                    .get_duration("delay_length")
                    .unwrap_or(Duration::from_secs(1));

                Ok(AddNode::DelayWriteStereo {
                    delay_name: delay_name,
                    delay_length: delay_length,
                })
            }
            "delay_read_mono" => {
                let p = params
                    .ok_or(ValidationError::MissingRequiredParameter(String::from(
                        "Delay read mono requires delay key",
                    )))
                    .unwrap();

                let p_list = param_list!("delay_name", "offsets");

                p.validate(&p_list).unwrap();
                p.required(&param_list!("delay_name")).unwrap();

                let delay_name = p.get_str("delay_name").unwrap();
                let offsets = p
                    .get_array_duration_ms("offsets")
                    .unwrap_or(vec![Duration::from_millis(200); 1]);

                Ok(AddNode::DelayReadMono {
                    delay_name: delay_name,
                    offsets: offsets,
                })
            }
            "delay_read_stereo" => {
                let p = params
                    .ok_or(ValidationError::MissingRequiredParameter(String::from(
                        "Delay read stereo requires delay key",
                    )))
                    .unwrap();

                let p_list = param_list!("delay_name", "offsets");

                p.validate(&p_list).unwrap();
                p.required(&param_list!("delay_name")).unwrap();

                let delay_name = p.get_str("delay_name").unwrap();
                let offsets = p
                    .get_array_duration_ms("offsets")
                    .unwrap_or(vec![Duration::from_millis(200); 1]);

                Ok(AddNode::DelayReadStereo {
                    delay_name: delay_name,
                    offsets: offsets,
                })
            }
            // FIR filters
            "fir_mono" => {
                let p = params.ok_or(ValidationError::MissingRequiredParameter(
                    "fir_mono requires coeffs".into(),
                ))?;
                let allowed = param_list!("coeffs");
                p.validate(&allowed)?;
                p.required(&allowed)?;

                let coeffs = p.get_array_f32("coeffs").unwrap();
                Ok(AddNode::FirMono { coeffs })
            }

            "fir_stereo" => {
                let p = params.ok_or(ValidationError::MissingRequiredParameter(
                    "fir_stereo requires coeffs".into(),
                ))?;
                let allowed = param_list!("coeffs");
                p.validate(&allowed)?;
                p.required(&allowed)?;

                let coeffs = p.get_array_f32("coeffs").unwrap();
                Ok(AddNode::FirStereo { coeffs })
            }
            // Ops
            "add_mono" => {
                if let Some(p) = params {
                    p.validate(&param_list!("val"))?;
                }
                let props = params.and_then(|p| p.get_f32("val")).unwrap_or(1.0);
                Ok(AddNode::AddMono { props })
            }

            "add_stereo" => {
                if let Some(p) = params {
                    p.validate(&param_list!("val"))?;
                }
                let props = params.and_then(|p| p.get_f32("val")).unwrap_or(1.0);
                Ok(AddNode::AddStereo { props })
            }

            "mult_mono" => {
                if let Some(p) = params {
                    let p_list = param_list!("val");
                    p.validate(&p_list)?;
                    p.required(&p_list)?;
                }
                let props = params.and_then(|p| p.get_f32("val")).unwrap_or(1.0);
                println!("{:?}", props);
                Ok(AddNode::MultMono { props })
            }

            "mult_stereo" => {
                if let Some(p) = params {
                    let p_list = param_list!("val");
                    p.validate(&p_list)?;
                    p.required(&p_list)?;
                }
                let props = params.and_then(|p| p.get_f32("val")).unwrap_or(1.0);
                println!("{:?}", props);
                Ok(AddNode::MultStereo { props })
            }

            // Mixers
            "stereo_mixer" => Ok(AddNode::StereoMixer),
            "stereo_to_mono" => Ok(AddNode::StereoToMono),
            "two_track_stereo_mixer" => Ok(AddNode::TwoTrackStereoMixer),
            "four_track_stereo_mixer" => Ok(AddNode::FourTrackStereoMixer),
            "eight_track_stereo_mixer" => Ok(AddNode::EightTrackStereoMixer),
            "two_track_mono_mixer" => Ok(AddNode::TwoTrackMonoMixer),
            "four_to_mono_mixer" => Ok(AddNode::FourToMonoMixer),

            // Sweep
            "sweep" => {
                let p = params.ok_or(ValidationError::MissingRequiredParameter(
                    "sweep requires range and duration".into(),
                ))?;

                let allowed = param_list!("range", "duration");
                p.validate(&allowed)?;
                p.required(&allowed)?;

                let range_vals =
                    p.get_array_f32("range")
                        .ok_or(ValidationError::InvalidParameter(
                            "range must be [start,end]".into(),
                        ))?;

                if range_vals.len() != 2 {
                    return Err(ValidationError::InvalidParameter(
                        "range must contain exactly two f32 values".into(),
                    ));
                }

                let duration =
                    p.get_duration("duration")
                        .ok_or(ValidationError::InvalidParameter(
                            "duration required".into(),
                        ))?;

                Ok(AddNode::Sweep {
                    range: (range_vals[0], range_vals[1]),
                    duration,
                })
            }
            _ => Err(ValidationError::NodeNotFound(format!(
                "Could not find node with name {}",
                name
            ))),
        }
    }
}
