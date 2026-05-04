use crate::{
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
    spec::{NodeDefinition, NodeDoc},
};

pub fn audio_node_docs() -> Vec<NodeDoc> {
    vec![
        Sine::doc(),
        Sampler::doc(),
        DelayWrite::doc(),
        DelayRead::doc(),
        TrackMixer::doc(),
        MultDef::doc(),
        AddDef::doc(),
        SubDef::doc(),
        DivDef::doc(),
        GainDef::doc(),
        Adsr::doc(),
        Svf::doc(),
        MonoFanOut::doc(),
        Sweep::doc(),
        OnePole::doc(),
        Allpass::doc(),
        HadamardMixer::doc(),
        HouseholderMixer::doc(),
        ExternalInput::doc(),
    ]
}

pub fn control_node_docs() -> Vec<NodeDoc> {
    vec![
        Signal::doc(),
        Map::doc(),
        Phasor::doc(),
        ClockDef::doc(),
        StepSequencer::doc(),
    ]
}

pub fn midi_node_docs() -> Vec<NodeDoc> {
    vec![Voice::doc(), PolyVoice::doc()]
}

/// Returns documentation for all built-in nodes across audio, control, and MIDI namespaces.
pub fn all_node_docs() -> Vec<NodeDoc> {
    let mut docs = audio_node_docs();
    docs.extend(control_node_docs());
    docs.extend(midi_node_docs());
    docs
}

/// Serialises all built-in node documentation to JSON
#[cfg(feature = "docs")]
pub fn export_nodes_json() -> String {
    serde_json::to_string_pretty(&all_node_docs()).expect("NodeDoc serialisation failed")
}
