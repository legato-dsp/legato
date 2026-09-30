use std::time::Instant;

use slotmap::new_key_type;

use crate::{
    config::Config,
    midi::{MidiError, MidiEvent, MidiMessage, MidiRuntimeFrontend, MidiStore, frame_since},
    resources::{
        Resources,
        params::{ParamError, ParamKey},
    },
};

new_key_type! { struct ExternalAudioKey; }

const MIDI_STORE_CAPACITY: usize = 256;

/// The AudioContext struct contains information about the current audio graph, as well as
/// some resources that are hosted up for nodes to access within a specific runtime.
///
/// This prevents complex state sharing or unsafe ptr logic when using things like shared buffers
/// for delay lines or samples.
pub struct AudioContext {
    config: Config,
    midi_store: MidiStore,
    // Filled by the host while the next block accumulates, see [`AudioContext::write_midi`]
    midi_pending: MidiStore,
    midi_runtime_frontend: Option<MidiRuntimeFrontend>,
    resources: Resources,
    block_start: Instant,
}

impl AudioContext {
    pub fn new(config: Config, resources: Resources) -> Self {
        Self {
            config,
            midi_store: MidiStore::new(MIDI_STORE_CAPACITY),
            midi_pending: MidiStore::new(MIDI_STORE_CAPACITY),
            resources,
            midi_runtime_frontend: None,
            block_start: Instant::now(),
        }
    }
    /// For a time being, this is a quick hack inside oversampling. I would recommend not using, as it does not reflex internal state!!!
    pub fn set_sample_rate(&mut self, sr: usize) {
        self.config.sample_rate = sr;
    }

    /// Make the host-written events current, then add live events stamped against the
    /// end of the previous block, trading one block of latency for jitter-free timing.
    pub(crate) fn begin_midi_block(&mut self) {
        std::mem::swap(&mut self.midi_store, &mut self.midi_pending);
        self.midi_pending.clear();

        let Some(runtime) = &self.midi_runtime_frontend else {
            return;
        };

        let Config {
            sample_rate,
            block_size,
            ..
        } = self.config;

        while let Some((msg, at)) = runtime.recv() {
            let frame = frame_since(self.block_start, at, sample_rate, block_size);
            if let Err(e) = self.midi_store.insert(MidiEvent { frame, msg }) {
                eprintln!("{:?}", e);
            }
        }
    }

    /// Queue an event for frame `frame` of the next block.
    #[inline(always)]
    pub fn write_midi(&mut self, frame: usize, msg: MidiMessage) -> Result<(), MidiError> {
        let frame = frame.min(self.config.block_size.saturating_sub(1)) as u32;
        self.midi_pending.insert(MidiEvent { frame, msg })
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

    pub fn set_midi_runtime_frontend(&mut self, frontend: MidiRuntimeFrontend) {
        self.midi_runtime_frontend = Some(frontend)
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
    pub fn get_midi_store(&self) -> &MidiStore {
        &self.midi_store
    }

    #[inline(always)]
    pub fn set_instant(&mut self) {
        self.block_start = Instant::now()
    }

    #[inline(always)]
    pub fn get_instant(&self) -> Instant {
        self.block_start
    }
}
