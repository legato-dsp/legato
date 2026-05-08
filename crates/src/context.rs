use std::time::Instant;

use slotmap::new_key_type;

use crate::{
    config::Config,
    midi::{MidiError, MidiMessage, MidiRuntimeFrontend, MidiStore},
    resources::{
        Resources,
        params::{ParamError, ParamKey},
    },
};

new_key_type! { struct ExternalAudioKey; }

/// The AudioContext struct contains information about the current audio graph, as well as
/// some resources that are hosted up for nodes to access within a specific runtime.
///
/// This prevents complex state sharing or unsafe ptr logic when using things like shared buffers
/// for delay lines or samples.
pub struct AudioContext {
    config: Config,
    midi_store: Option<MidiStore>,
    midi_runtime_frontend: Option<MidiRuntimeFrontend>,
    resources: Resources,
    block_start: Instant,
}

impl AudioContext {
    pub fn new(config: Config, resources: Resources) -> Self {
        Self {
            config,
            midi_store: None,
            resources,
            midi_runtime_frontend: None,
            block_start: Instant::now(),
        }
    }
    /// For a time being, this is a quick hack inside oversampling. I would recommend not using, as it does not reflex internal state!!!
    pub fn set_sample_rate(&mut self, sr: usize) {
        self.config.sample_rate = sr;
    }

    pub(crate) fn update_midi(&mut self) {
        self.clear_midi();

        let Some(runtime) = self.midi_runtime_frontend.take() else {
            return;
        };

        while let Some(msg) = runtime.recv() {
            if let Err(e) = self.insert_midi_msg(msg) {
                eprintln!("{:?}", e);
            }
        }

        self.midi_runtime_frontend = Some(runtime);
    }

    /// For a time being, this is a quick hack inside oversampling. I would recommend not using, as it does not reflex internal state!!!
    pub fn set_block_size(&mut self, block_size: usize) {
        self.config.block_size = block_size;
    }

    pub fn get_config(&self) -> Config {
        self.config
    }

    pub fn get_resources(&self) -> &Resources {
        &self.resources
    }

    pub fn set_resources(&mut self, resources: Resources) {
        self.resources = resources;
    }

    pub fn get_resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
    }

    pub fn get_param(&self, key: &ParamKey) -> Result<f32, ParamError> {
        self.resources.get_param(key)
    }

    pub fn sample_rate_f32(&self) -> f32 {
        self.config.sample_rate as f32
    }

    #[inline(always)]
    pub fn sample_rate(&self) -> usize {
        self.config.sample_rate
    }

    // Add a midi store to the runtime.
    pub fn set_midi_store(&mut self, store: MidiStore) {
        self.midi_store = Some(store);
    }

    pub fn send_to_system_midi(
        &mut self,
        msg: MidiMessage,
        instant: Instant,
    ) -> Result<(), MidiError> {
        if let Some(inner) = &mut self.midi_runtime_frontend {
            inner.writer_frontend.send_to_system_midi(msg, instant)
        } else {
            Err(MidiError::MissingRuntime)
        }
    }

    #[inline(always)]
    pub fn get_midi_store(&self) -> Option<&MidiStore> {
        self.midi_store.as_ref()
    }

    #[inline(always)]
    pub fn set_instant(&mut self) {
        self.block_start = Instant::now()
    }

    #[inline(always)]
    pub fn get_instant(&self) -> Instant {
        self.block_start
    }

    /// Insert a midi message into the store.
    #[inline(always)]
    pub fn insert_midi_msg(&mut self, msg: MidiMessage) -> Result<(), MidiError> {
        let store = self.midi_store.as_mut().unwrap();
        store.insert(msg)
    }

    #[inline(always)]
    pub fn clear_midi(&mut self) {
        if let Some(store) = &mut self.midi_store {
            store.clear();
        }
    }
}
