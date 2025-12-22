#![allow(unused_mut)]

use std::{collections::HashMap, time::Duration};

use crate::{
    ast::DSLParams,
    builder::{ResourceBuilderView, ValidationError},
    node::DynNode,
    node_spec,
    nodes::{
        audio::{
            delay::{DelayLine, DelayRead, DelayWrite},
            mixer::TrackMixer,
            ops::{ApplyOpKind, mult_node_factory},
            sampler::Sampler,
            sine::Sine,
            svf::{FilterType, Svf},
            sweep::Sweep,
        },
        control::signal::Signal,
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
            optional = ["chans"],
            build = |_, p| {
                let chans = p.get_usize("chans").unwrap_or(2);
                let val = p.get_f32("val").unwrap_or(1.0);

                let node = mult_node_factory(val, chans, ApplyOpKind::Mult);

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
    ]);
    NodeRegistry { data }
}

pub fn control_registry_factory() -> NodeRegistry {
    let mut data = HashMap::new();
    data.extend([node_spec!(
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
    )]);
    NodeRegistry { data }
}
