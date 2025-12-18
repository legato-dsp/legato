use std::{collections::HashMap, time::Duration};

use crate::{
    ValidationError,
    builder::ResourceBuilderView,
    node::DynNode,
    node_spec,
    nodes::audio::{
        delay::{DelayLine, DelayRead, DelayWrite},
        mixer::TrackMixer,
        ops::{ApplyOpKind, mult_node_factory},
        sampler::Sampler,
        sine::Sine,
        sweep::Sweep,
    },
    params::Params,
    spec::{NodeFactory, NodeSpec},
};

/// Audio registries are simply hashmaps of String node names, and their
/// corresponding NodeSpec.
///
/// This lets Legato users add additional nodes to a "namespace" of nodes.

pub struct AudioRegistry {
    data: HashMap<String, NodeSpec>,
}

impl AudioRegistry {
    pub fn new() -> Self {
        let data = HashMap::new();
        Self { data }
    }
    pub fn get_node(
        &self,
        resource_builder: &mut ResourceBuilderView,
        node_kind: &String,
        params: &Params,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        match self.data.get(node_kind) {
            Some(spec) => (spec.build)(resource_builder, params),
            None => Err(ValidationError::NodeNotFound(format!(
                "Could not find node {}",
                node_kind
            ))),
        }
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

impl Default for AudioRegistry {
    fn default() -> Self {
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
                        .expect(&format!("Could not find delay line key {}", name));

                    let node = DelayRead::new(chans, key, len);

                    Ok(Box::new(node))
                }
            ),
            node_spec!(
                "track_mixer".into(),
                required = ["tracks", "chans_per_track"],
                optional = ["gain"],
                build = |_, p| {
                    let chans_per_track = p.get_usize("chans_per_track").expect(
                        "Could not find required parameter chans_per_track for track mixer!",
                    );
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
        Self { data }
    }
}
