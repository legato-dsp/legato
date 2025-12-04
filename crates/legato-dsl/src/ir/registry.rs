use std::{
    collections::{HashMap},
    time::Duration,
};

use legato_core::runtime::builder::AddNode;

use crate::ir::node_spec::NodeSpec;
use crate::node_spec;

pub struct AudioRegistry(HashMap<&'static str, NodeSpec>);

impl Default for AudioRegistry {
    fn default() -> Self {
        let mut table = HashMap::new();
        table.extend([
            node_spec!(
                "sine",
                required = [],
                optional = ["freq", "chans"],
                build = |p| {
                    let freq = p.get_f32("freq").unwrap_or(440.0);
                    let chans = p.get_usize("chans").unwrap_or(1);
                    Ok(AddNode::Sine { freq, chans })
                }
            ),
            node_spec!(
                "sampler",
                required = ["sampler_name"],
                optional = ["chans"],
                build = |p| {
                    let name = p.get_str("sampler_name").unwrap();
                    let chans = p.get_usize("chans").unwrap_or(1);
                    Ok(AddNode::Sampler { sampler_name: name, chans })
                }
            ),
            node_spec!(
                "delay_write",
                required = ["delay_name"],
                optional = ["delay_length", "chans"],
                build = |p| {
                    let name = p.get_str("delay_name").unwrap();
                    let len = p.get_duration("delay_length")
                        .unwrap_or(Duration::from_secs(1));
                    let chans = p.get_usize("chans").unwrap_or(1);
                    Ok(AddNode::DelayWrite {
                        delay_name: name,
                        delay_length: len,
                        chans,
                    })
                }
            ),
        ]);
        Self(table)
    }
}
