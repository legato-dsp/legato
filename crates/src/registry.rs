#![allow(unused_mut)]

use std::{collections::HashMap, time::Duration};

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    ir::DSLParams,
    node::DynNode,
    node_spec,
    nodes::{
        audio::{
            adsr::Adsr,
            allpass::Allpass,
            delay::{DelayLine, DelayRead, DelayWrite},
            mixer::{MonoFanOut, TrackMixer},
            ops::{ApplyOpKind, mult_node_factory},
            sampler::Sampler,
            sine::Sine,
            svf::{FilterType, Svf},
            sweep::Sweep,
        },
        control::{map::Map, signal::Signal},
        midi::voice::{PolyVoice, Voice},
    },
    params::ParamMeta,
    spec::{NodeFactory, NodeSpec},
};

/// Node registries are simply hashmaps of String node names, and their
/// corresponding NodeSpec.
///
/// This lets Legato users add additional nodes to a "namespace" of nodes.
pub struct NodeRegistry {
    // For now, entries must contain a specific rate as I work out graph semantics
    data: HashMap<String, NodeSpec>,
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeRegistry {
    pub fn new() -> Self {
        let data = HashMap::new();
        Self { data }
    }
    pub fn get_node(
        &self,
        resource_builder: &mut ResourceBuilderView,
        node_name: &String,
        params: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let node = match self.data.get(node_name) {
            Some(spec) => (spec.build)(resource_builder, params),
            None => Err(ValidationError::NodeNotFound(format!(
                "Could not find node {}",
                node_name
            ))),
        }?;
        Ok(node)
    }
    pub fn declare_node(&mut self, spec: NodeSpec) {
        self.data
            .insert(spec.name.clone(), spec)
            .expect("Could not declare node!");
    }
}

pub fn get_spec_for_runtime(name: String, runtime_factory: NodeFactory) -> (String, NodeSpec) {
    node_spec!(
        name.clone(),
        required = [],
        optional = [],
        build = runtime_factory
    )
}

pub fn audio_registry_factory() -> NodeRegistry {
    let mut data = HashMap::new();
    data.extend([
        node_spec!(
            "sine".into(),
            required = [],
            optional = ["freq", "chans"],
            build = |_, p| {
                let freq = p.get_f32("freq").unwrap_or(440.0);
                let chans = p.get_usize("chans").unwrap_or(2);
                Ok(Box::new(Sine::new(freq, chans)))
            }
        ),
        node_spec!(
            "sampler".into(),
            required = ["sampler_name"],
            optional = ["chans"],
            build = |rb, p| {
                let name = p
                    .get_str("sampler_name")
                    .expect("Could not find required parameter sampler_name");
                let chans = p.get_usize("chans").unwrap_or(2);

                let key = rb.add_sampler(&name);

                let node = Sampler::new(key, chans);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "delay_write".into(),
            required = ["delay_name"],
            optional = ["delay_length", "chans"],
            build = |rb, p| {
                let name = p
                    .get_str("delay_name")
                    .expect("Could not find required parameter delay_name");

                let len = p
                    .get_duration("delay_length")
                    .unwrap_or(Duration::from_secs(1));

                let chans = p.get_usize("chans").unwrap_or(2);

                let sr = rb.get_config().sample_rate as f32;
                let capacity = sr * len.as_secs_f32();

                let delay_line = DelayLine::new(capacity as usize, chans);

                let key = rb.add_delay_line(&name, delay_line);

                let node = DelayWrite::new(key, chans);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "delay_read".into(),
            required = ["delay_name"],
            optional = ["delay_length", "chans"],
            build = |rb, p| {
                let name = p
                    .get_str("delay_name")
                    .expect("Could not find required parameter sampler_name");
                let len = p
                    .get_array_duration_ms("delay_length")
                    .unwrap_or(vec![Duration::from_secs(1); 2]);

                let chans = p.get_usize("chans").unwrap_or(2);

                let key = rb
                    .get_delay_line_key(&name)
                    .unwrap_or_else(|_| panic!("Could not find delay line key {}", name));

                let node = DelayRead::new(chans, key, len);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "track_mixer".into(),
            required = ["tracks", "chans_per_track"],
            optional = ["gain"],
            build = |_, p| {
                let chans_per_track = p
                    .get_usize("chans_per_track")
                    .expect("Could not find required parameter chans_per_track for track mixer!");
                let tracks = p
                    .get_usize("tracks")
                    .expect("Could not find required parameter tracks for track mixer!");
                let gain = p
                    .get_array_f32("gain")
                    .unwrap_or(vec![(1.0 / f32::sqrt(tracks as f32))]);

                let node = TrackMixer::new(chans_per_track, tracks, gain);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "mult".into(),
            required = ["val"],
            optional = [],
            build = |_, p| {
                let val = p.get_f32("val").unwrap_or(1.0);

                let node = mult_node_factory(val, 1, ApplyOpKind::Mult);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "add".into(),
            required = ["val"],
            optional = [],
            build = |_, p| {
                let val = p.get_f32("val").unwrap_or(0.0);

                let node = mult_node_factory(val, 1, ApplyOpKind::Add);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "sub".into(),
            required = ["val"],
            optional = [],
            build = |_, p| {
                let val = p.get_f32("val").unwrap_or(0.0);

                let node = mult_node_factory(val, 1, ApplyOpKind::Subtract);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "div".into(),
            required = ["val"],
            optional = [],
            build = |_, p| {
                let val = p.get_f32("val").unwrap_or(0.0);

                let node = mult_node_factory(val, 1, ApplyOpKind::Div);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "gain".into(),
            required = ["val"],
            optional = ["chans"],
            build = |_, p| {
                let chans = p.get_usize("chans").unwrap_or(2);
                let val = p.get_f32("val").unwrap_or(1.0);

                let node = mult_node_factory(val, chans, ApplyOpKind::Gain);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "adsr".into(),
            required = ["attack", "decay", "sustain", "release", "chans"],
            optional = [],
            build = |_, p| {
                let attack = p.get_f32("attack").expect("Must provide attack to ADSR");
                let decay = p.get_f32("decay").expect("Must provide decay to ADSR");
                let sustain = p.get_f32("sustain").expect("Must provide sustain to ADSR");
                let release = p.get_f32("release").expect("Must provide release to ADSR");
                let chans = p.get_usize("chans").expect("Must provide chans to ADSR");

                let node = Adsr::new(chans, attack, decay, sustain, release);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "svf".into(),
            required = [],
            optional = ["cutoff", "q", "type", "chans"],
            build = |rb, p| {
                let cutoff = p.get_f32("cutoff").unwrap_or(7500.0);
                let chans = p.get_usize("chans").unwrap_or(2);
                let gain = p.get_f32("gain").unwrap_or(1.0);
                let q = p.get_f32("q").unwrap_or(0.4);

                let filter_type =
                    p.get_str("type")
                        .map_or(FilterType::LowPass, |f| match f.as_str() {
                            "lowpass" => FilterType::LowPass,
                            "highpass" => FilterType::HighPass,
                            "allpass" => FilterType::AllPass,
                            "bandpass" => FilterType::BandPass,
                            "bell" => FilterType::Bell,
                            "highshelf" => FilterType::HighShelf,
                            "notch" => FilterType::Notch,
                            "peak" => FilterType::Peak,
                            _ => panic!("Could not find filter type!"),
                        });

                let sr = rb.config.sample_rate as f32;
                let node = Svf::new(sr, filter_type, cutoff, gain, q, chans);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "mono_fan_out".into(),
            required = [],
            optional = ["chans"],
            build = |_, p| {
                let chans = p.get_usize("chans").unwrap_or(2);
                let node = MonoFanOut::new(chans);

                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "sweep".into(),
            required = [],
            optional = ["duration", "range", "chans"],
            build = |_, p| {
                let chans = p.get_usize("chans").unwrap_or(2);
                let duration = p
                    .get_duration("duration")
                    .unwrap_or(Duration::from_secs_f32(5.0));
                let range = p.get_array_f32("range").unwrap_or([40., 48_000.].into());

                let node = Sweep::new(*range.as_array().unwrap(), duration, chans);
                Ok(Box::new(node))
            }
        ),
        node_spec!(
            "allpass".into(),
            required = ["delay_length", "feedback", "chans"], // TODO: Something more eloquent for capacity
            optional = ["capacity"],
            build = |rb, p| {
                let config = rb.get_config();

                let sr = config.sample_rate;

                let chans = p.get_usize("chans").unwrap_or(2);

                let delay_length = p
                    .get_duration("delay_length")
                    .unwrap_or(Duration::from_millis(200));

                let delay_length_samples = sr as f32 * (delay_length.as_secs_f32());

                let feedback = p.get_f32("feedback").unwrap_or(0.5);

                let mut capacity = p.get_usize("capacity").unwrap_or(sr * 1);

                // Clamp with reasonable allpass size.
                if capacity < (delay_length_samples as usize) {
                    capacity = (delay_length_samples as usize) * 2;
                }

                let node = Allpass::new(chans, feedback, delay_length_samples, capacity);

                Ok(Box::new(node))
            }
        ),
    ]);
    NodeRegistry { data }
}

pub fn control_registry_factory() -> NodeRegistry {
    let mut data = HashMap::new();
    data.extend([
        node_spec!(
            "signal".into(),
            required = ["name", "min", "max", "default"],
            optional = ["smoothing"],
            build = |rb, p| {
                let name = p.get_str("name").expect("Must pass name to signal!");
                let min = p.get_f32("min").expect("Must provide min to signal!");
                let max = p.get_f32("max").expect("Must provide max to signal!");
                let default = p
                    .get_f32("default")
                    .expect("Must provide default(f32) to signal!");

                let smoothing = p.get_f32("smoothing").unwrap_or(0.5).clamp(0.0, 1.0);

                let meta = ParamMeta {
                    name: name.clone(),
                    min,
                    max,
                    default,
                };

                let key = rb.add_param(name, meta);

                Ok(Box::new(Signal::new(key, default, smoothing)))
            }
        ),
        node_spec!(
            "map".into(),
            required = ["range", "new_range "],
            optional = [],
            build = |_, p| {
                let range = p
                    .get_array_f32("range")
                    .expect("Must pass original range to map");
                let new_range = p
                    .get_array_f32("new_range")
                    .expect("Must pass new_range to map");

                // Make sure range is correct length
                assert!(range.len() == 2);
                assert!(new_range.len() == 2);

                // Probably a nicer way to do this

                let mut r_0 = [0.0; 2];
                let mut r_1 = [0.0; 2];

                for i in 0..2 {
                    r_0[i] = range[i];
                    r_1[i] = new_range[i]
                }

                Ok(Box::new(Map::new(r_0, r_1)))
            }
        ),
    ]);
    NodeRegistry { data }
}

pub fn midi_registry_factory() -> NodeRegistry {
    let mut data = HashMap::new();
    data.extend([
        node_spec!(
            "voice".into(),
            required = ["chan"],
            optional = [],
            build = |_, p| {
                let channel = p
                    .get_usize("chan")
                    .expect("Must provide midi channel (chan) (0-15) to voice!");
                assert!(channel <= 15);
                Ok(Box::new(Voice::new(channel)))
            }
        ),
        node_spec!(
            "poly_voice".into(),
            required = ["voices", "chan"],
            optional = [],
            build = |_, p| {
                let channel = p
                    .get_usize("chan")
                    .expect("Must provide midi channel (chan) (0-15) to voice!");
                let voices = p
                    .get_usize("voices")
                    .expect("Must provide number of voices to poly voice!");

                assert!(channel <= 15);
                assert!(
                    voices < 10,
                    "Currently, a maximum of 32 tracks is supported."
                );
                Ok(Box::new(PolyVoice::new(voices, channel)))
            }
        ),
    ]);
    NodeRegistry { data }
}
