use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use legato_core::{nodes::Node, runtime::{builder::AddNode, runtime::Runtime}};

use crate::ir::{ValidationError, node_spec::NodeSpec, params::Params};
use crate::node_spec;

pub struct AudioRegistry {
    data: HashMap<String, NodeSpec>
}

impl AudioRegistry {
    pub fn new() -> Self {
        let data = HashMap::new();
        Self {
            data
        }
    }
    pub fn get_node(&self, name: &String, params: Option<&Params>) -> Result<AddNode, ValidationError> {
        if let Some(p) = params {
            return match self.data.get(name) {
                Some(spec) => (spec.build)(p),
                None => Err(ValidationError::NodeNotFound(format!("Could not find node {}", name)))
            }
        }
        let temp = BTreeMap::new();
        let p = Params(&temp);
        match self.data.get(name) {
            Some(spec) => (spec.build)(&p),
            None => Err(ValidationError::NodeNotFound(format!("Could not find node {}", name)))
        }
    }
    pub fn declare_node(&mut self, name: String, spec: NodeSpec) {
        self.data.insert(name, spec).expect("Could not declare node!");
    }
}

pub fn get_spec_for_runtime(name: String, runtime: Box<Runtime>) -> (String, NodeSpec) {
    let spec = node_spec!(
        name,
        required = [],
        optional = [],
        build = move |_| Ok(AddNode::Subgraph { runtime })
        
    );
    spec
}

impl Default for AudioRegistry {
    fn default() -> Self {
        let mut data = HashMap::new();
        data.extend([
            node_spec!(
                "sine".into(),
                required = [],
                optional = ["freq", "chans"],
                build = |p| {
                    let freq = p.get_f32("freq").unwrap_or(440.0);
                    let chans = p.get_usize("chans").unwrap_or(2);
                    Ok(AddNode::Sine { freq, chans })
                }
            ),
            node_spec!(
                "sampler".into(),
                required = ["sampler_name"],
                optional = ["chans"],
                build = |p| {
                    let name = p.get_str("sampler_name").expect("Could not find required parameter sampler_name");
                    let chans = p.get_usize("chans").unwrap_or(2);
                    Ok(AddNode::Sampler { sampler_name: name, chans })
                }
            ),
            node_spec!(
                "delay_write".into(),
                required = ["delay_name"],
                optional = ["delay_length", "chans"],
                build = |p| {
                    let name = p.get_str("delay_name").expect("Could not find required parameter delay_name");
                    let len = p.get_duration("delay_length")
                        .unwrap_or(Duration::from_secs(1));
                    let chans = p.get_usize("chans").unwrap_or(2);
                    Ok(AddNode::DelayWrite {
                        delay_name: name,
                        delay_length: len,
                        chans,
                    })
                }
            ),
            node_spec!(
                "delay_read".into(),
                required = ["delay_name"],
                optional = ["delay_length", "chans"],
                build = |p| {
                    let name = p.get_str("delay_name").expect("Could not find required parameter sampler_name");
                    let len = p.get_array_duration_ms("delay_length")
                        .unwrap_or(vec![Duration::from_secs(1); 2]);
                    let chans = p.get_usize("chans").unwrap_or(2);
                    Ok(AddNode::DelayRead {
                        delay_name: name,
                        delay_length: len,
                        chans,
                    })
                }
            ),
            node_spec!(
                "track_mixer".into(),
                required = ["tracks", "chans_per_track"],
                optional = ["gain"],
                build = |p| {
                    let chans_per_track = p.get_usize("chans_per_track").expect("Could not find required parameter chans_per_track for track mixer!");
                    let tracks = p.get_usize("tracks").expect("Could not find required parameter tracks for track mixer!");
                    let gain = p.get_array_f32("gain").unwrap_or(vec![(1.0 / f32::sqrt(tracks as f32))]);
                    Ok(
                        AddNode::TrackMixer { chans_per_track: chans_per_track, tracks: tracks, gain:  gain}
                    )
                }
            ),
            node_spec!(
                "mult".into(),
                required = ["val"],
                optional = ["chans"],
                build = |p| {
                    let chans = p.get_usize("chans").unwrap_or(2);
                    let val = p.get_f32("val").unwrap_or(1.0);
                    Ok(
                        AddNode::Mult { val, chans }
                    )
                }
            ),
            node_spec!(
                "gain".into(),
                required = ["val"],
                optional = ["chans"],
                build = |p| {
                    let chans = p.get_usize("chans").unwrap_or(2);
                    let val = p.get_f32("val").unwrap_or(1.0);
                    Ok(
                        AddNode::Gain { val, chans }
                    )
                }
            ),
        ]);
        Self { data }
    }
}
