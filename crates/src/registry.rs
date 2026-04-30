#![allow(unused_mut)]

use std::collections::HashMap;

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    node_spec,
    nodes::{
        audio::{
            adsr::Adsr,
            allpass::Allpass,
            delay::{DelayRead, DelayWrite},
            external::ExternalInput,
            hadamard::HadamardMixer,
            householder::HouseholderMixer,
            mixer::{MonoFanOut, TrackMixer},
            onepole::OnePole,
            ops::{AddDef, DivDef, GainDef, MultDef, SubDef},
            sampler::Sampler,
            sine::Sine,
            svf::Svf,
            sweep::Sweep,
        },
        control::{
            map::Map,
            phasor::{ClockDef, Phasor},
            sequencer::StepSequencer,
            signal::Signal,
        },
        midi::voice::{PolyVoice, Voice},
    },
    spec::{NodeDefinition, NodeSpec},
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
            Some(spec) => {
                spec.check_for_bad_params(params);
                (spec.build)(resource_builder, params)
            }
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

    /// Register a node type that implements [`NodeDefinition`].
    pub fn register_node<T: NodeDefinition>(&mut self) {
        let spec = T::spec();
        self.data.insert(spec.name.clone(), spec);
    }
}

// Here, we assemble all of these registries. I might look into linking/static graphs in the future instead.

pub fn audio_registry_factory() -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    registry.register_node::<Sine>();
    registry.register_node::<Sampler>();
    registry.register_node::<DelayWrite>();
    registry.register_node::<DelayRead>();
    registry.register_node::<TrackMixer>();
    registry.register_node::<MultDef>();
    registry.register_node::<AddDef>();
    registry.register_node::<SubDef>();
    registry.register_node::<DivDef>();
    registry.register_node::<GainDef>();
    registry.register_node::<Adsr>();
    registry.register_node::<Svf>();
    registry.register_node::<MonoFanOut>();
    registry.register_node::<Sweep>();
    registry.register_node::<OnePole>();
    registry.register_node::<Allpass>();
    registry.register_node::<HadamardMixer>();
    registry.register_node::<HouseholderMixer>();
    registry.register_node::<ExternalInput>();
    registry
}

pub fn control_registry_factory() -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    registry.register_node::<Signal>();
    registry.register_node::<Map>();
    registry.register_node::<Phasor>();
    registry.register_node::<ClockDef>();
    registry.register_node::<StepSequencer>();
    registry
}

pub fn midi_registry_factory() -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    registry.register_node::<Voice>();
    registry.register_node::<PolyVoice>();
    registry
}
