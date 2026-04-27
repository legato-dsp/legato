use rtrb::{PushError, RingBuffer};
use slotmap::{SlotMap, new_key_type};
use std::collections::HashMap;

use crate::resources::{
    arena::{Arena, RuntimeArena},
    buffer::ExternalBuffer,
    delay::{DelayLineView, DelayLineViewMut, ResourceDelay},
    input::AudioInput,
    params::{ParamError, ParamKey, ParamMeta, ParamStore, ParamStoreBuilder, ParamStoreFrontend},
    window::Window,
};

pub mod arena;
pub mod buffer;
pub mod delay;
pub mod input;
pub mod params;
pub mod window;

new_key_type! { pub struct InternalBufferKey; }
new_key_type! {pub struct ExternalBufferKey; }
new_key_type! {pub struct AudioInputKey; }
new_key_type! { pub struct DelayLineKey; }

#[derive(Debug, Default)]
pub struct ExternalBufferUpdate {
    pub key: ExternalBufferKey,
    pub buffer: ExternalBuffer,
}

/// Resources are shared resources for the entire runtime.
///
/// This let's us hoist up shared delay lines, LUT, granular buffers, etc.
///
/// There are effectively two types of runtime resources.
///
/// - internal resources: These are preallocated, or can be allocated at runtime on a
///   slice of the [`RuntimeArena`], provided we have already allocated this and still have room.
///
/// - external resources: Loaded on another thread, and send with producer
pub struct Resources {
    arena: RuntimeArena,
    param_store: ParamStore,
    internal_buffers: SlotMap<InternalBufferKey, Window>,
    external_buffers: SlotMap<ExternalBufferKey, Option<ExternalBuffer>>,
    delay_lines: SlotMap<DelayLineKey, ResourceDelay>,
    external_buffer_update_receiver: rtrb::Consumer<ExternalBufferUpdate>,
    audio_inputs: SlotMap<AudioInputKey, AudioInput>,
    garbage_sender: rtrb::Producer<ExternalBuffer>,
}

impl Resources {
    pub fn new(
        arena: RuntimeArena,
        param_store: ParamStore,
        receiver: rtrb::Consumer<ExternalBufferUpdate>,
        garbage_sender: rtrb::Producer<ExternalBuffer>,
        internal_buffers: SlotMap<InternalBufferKey, Window>,
        external_buffers: SlotMap<ExternalBufferKey, Option<ExternalBuffer>>,
        delay_lines: SlotMap<DelayLineKey, ResourceDelay>,
        audio_inputs: SlotMap<AudioInputKey, AudioInput>,
    ) -> Self {
        Self {
            arena,
            param_store,
            internal_buffers,
            external_buffers,
            delay_lines,
            external_buffer_update_receiver: receiver,
            audio_inputs,
            garbage_sender,
        }
    }

    pub fn delay_line_view(&self, key: DelayLineKey) -> DelayLineView<'_> {
        let delay = self.delay_lines.get(key).expect("Invalid delay key");
        let data = self.arena.slice(delay.get_window());
        DelayLineView { delay, data }
    }

    pub fn delay_line_view_mut(&mut self, key: DelayLineKey) -> DelayLineViewMut<'_> {
        let delay = self.delay_lines.get_mut(key).expect("Invalid delay key");
        let data = self.arena.slice_mut(delay.get_window());
        DelayLineViewMut { delay, data }
    }

    pub fn delay_line_cubic(&self, key: DelayLineKey, index: f32) -> f32 {
        let delay_line = self.delay_lines.get(key).expect("Invalid delay key");
        let window = delay_line.get_window();

        let buffer = self.arena.slice(window);
        delay_line.get_delay_cubic(buffer, index)
    }

    pub fn get_internal_buffer(&self, key: InternalBufferKey) -> Option<&[f32]> {
        match self.internal_buffers.get(key) {
            Some(w) => Some(self.arena.slice(*w)),
            None => None,
        }
    }

    pub fn get_internal_buffer_mut(&mut self, key: InternalBufferKey) -> Option<&mut [f32]> {
        match self.internal_buffers.get_mut(key) {
            Some(w) => Some(self.arena.slice_mut(*w)),
            None => None,
        }
    }

    pub fn get_external_buffer(&self, key: ExternalBufferKey) -> Option<&ExternalBuffer> {
        self.external_buffers.get(key).unwrap().as_ref()
    }

    pub fn get_external_buffer_mut(
        &mut self,
        key: ExternalBufferKey,
    ) -> Option<&mut ExternalBuffer> {
        self.external_buffers.get_mut(key).unwrap().as_mut()
    }

    #[inline(always)]
    pub fn get_param(&self, param_key: &ParamKey) -> Result<f32, ParamError> {
        self.param_store.get(param_key)
    }

    /// The entire buffer [l,l,l,r,r,r] for an audio input
    #[inline(always)]
    pub fn get_audio_input(&self, key: AudioInputKey) -> &[f32] {
        self.audio_inputs
            .get(key)
            .expect("Invalid AudioInputKey")
            .as_slice()
    }

    /// A single channel's samples for `key`.
    #[inline(always)]
    pub fn get_audio_input_chan(&self, key: AudioInputKey, chan: usize) -> &[f32] {
        self.audio_inputs
            .get(key)
            .expect("Invalid AudioInputKey")
            .channel(chan)
    }

    pub fn drain(&mut self) {
        // Drain external buffers
        while let Ok(incoming) = self.external_buffer_update_receiver.pop() {
            if let Some(buffer_ref) = self.external_buffers.get_mut(incoming.key) {
                // This returns the old value, and we can then clean it up in another thread
                let old_buffer = Option::replace(buffer_ref, incoming.buffer);
                if let Some(inner) = old_buffer
                    && self.garbage_sender.push(inner).is_err()
                {
                    panic!("Returned buffer was not sent to another thread to be dropped!")
                }
            }
        }

        // Drain the incoming audio receivers
        for (_, ai) in &mut self.audio_inputs {
            ai.drain();
        }
    }
}

#[derive(Default)]
pub struct ResourceBuilder {
    arena: Arena,
    /// Maps to a Window (used to create a slice) on our "arena"
    internal_buffers: SlotMap<InternalBufferKey, Window>,
    /// Maps to an optional external buffer
    external_buffers: SlotMap<ExternalBufferKey, Option<ExternalBuffer>>,
    /// Underlying hoisted DelayLine that is then constructed into a DelayLineView
    delay_lines: SlotMap<DelayLineKey, ResourceDelay>,
    external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
    // Register and store the AudioInputs
    audio_inputs: SlotMap<AudioInputKey, AudioInput>,
    audio_input_key_lookup: HashMap<String, AudioInputKey>,
    // RtSafe param store builder
    param_builder: ParamStoreBuilder,
}

impl ResourceBuilder {
    pub fn add_delay_line(&mut self, capacity: usize) -> DelayLineKey {
        let window = self.arena.alloc(capacity);
        let delay_line = ResourceDelay::new(window);

        self.delay_lines.insert(delay_line)
    }

    pub fn replace_delay_line(&mut self, key: DelayLineKey, capacity: usize) {
        let window = self.arena.alloc(capacity);
        let delay_line = ResourceDelay::new(window);
        *self.delay_lines.get_mut(key).expect("Delay line not found") = delay_line;
    }

    /// Register an external buffer, and add the window to the resource manager.
    pub fn add_external_buffer(
        &mut self,
        name: &str,
        buffer: Option<ExternalBuffer>,
    ) -> ExternalBufferKey {
        let key = self.external_buffers.insert(buffer);
        self.external_buffer_key_lookup.insert(name.into(), key);

        key
    }

    /// Register an internal buffer of a certain size, and add the window to the resource manager.
    pub fn add_internal_buffer(&mut self, size: usize) -> InternalBufferKey {
        let window = self.arena.alloc(size);
        self.internal_buffers.insert(window)
    }

    pub fn add_param(&mut self, unique_name: String, meta: ParamMeta) -> ParamKey {
        self.param_builder.add_param(unique_name, meta)
    }

    pub fn register_audio_input(
        &mut self,
        name: &str,
        consumer: rtrb::Consumer<f32>,
        chans: usize,
        block_size: usize,
    ) -> AudioInputKey {
        let input = AudioInput::new(chans, block_size, consumer);
        let key = self.audio_inputs.insert(input);
        self.audio_input_key_lookup.insert(name.into(), key);
        key
    }

    /// Look up an audio input key by name (for use in node factories).
    pub fn get_audio_input_key(&self, name: &str) -> Option<AudioInputKey> {
        self.audio_input_key_lookup.get(name).copied()
    }

    pub fn build(
        self,
        rt_capacity: usize,
        external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
    ) -> (ResourceFrontend, Resources) {
        let (param_frontend, store) = self.param_builder.build();
        // Seal the arena so we can no longer allocate. rt_capacity is the extra space alloted for RT allocation.
        let arena = self.arena.seal(rt_capacity);

        // TODO: Config, is this a sensible default?
        let (garbage_prod, garbage_cons) = RingBuffer::new(512);

        let (extern_buffer_update_prod, extern_buffer_update_cons) = RingBuffer::new(512);

        let resources = Resources::new(
            arena,
            store,
            extern_buffer_update_cons,
            garbage_prod,
            self.internal_buffers,
            self.external_buffers,
            self.delay_lines,
            self.audio_inputs,
        );

        let frontend = ResourceFrontend::new(
            param_frontend,
            extern_buffer_update_prod,
            garbage_cons,
            external_buffer_key_lookup,
        );

        (frontend, resources)
    }
}

/// Used to modify params, external samples, and receive samples
/// that need to be dropped on the non-realtime thread.
pub struct ResourceFrontend {
    /// Change params on the runtime
    param_front_end: ParamStoreFrontend,
    /// Send ['ExternalBufferUpdate'] to the runtime to update the buffer in the slot.
    external_sample_producer: rtrb::Producer<ExternalBufferUpdate>,
    /// Receive all of the [`ExternalBuffer`]'s that are then dropped on a non-realtime thread.
    external_sample_garbage_receiver: rtrb::Consumer<ExternalBuffer>,
    external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
}

impl ResourceFrontend {
    pub fn new(
        param_front_end: ParamStoreFrontend,
        external_sample_producer: rtrb::Producer<ExternalBufferUpdate>,
        external_sample_garbage_receiver: rtrb::Consumer<ExternalBuffer>,
        external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
    ) -> Self {
        Self {
            param_front_end,
            external_sample_producer,
            external_sample_garbage_receiver,
            external_buffer_key_lookup,
        }
    }

    /// Send an external buffer to the runtime
    pub fn send_external_buffer(
        &mut self,
        name: &str,
        buffer: ExternalBuffer,
    ) -> Result<(), PushError<ExternalBufferUpdate>> {
        let key = self
            .external_buffer_key_lookup
            .get(name)
            .unwrap_or_else(|| panic!("External buffer name {} not found!", name));

        let update = ExternalBufferUpdate { key: *key, buffer };
        self.external_sample_producer.push(update)
    }

    /// Garbage collect any external buffers that are no longer in use
    ///
    /// TODO: Find a consistent pattern to implement this
    /// We are doing this here, and not on the audio thread to avoid any non-realtime ops
    pub fn drain_garbage(&mut self) {
        while let Ok(garbage) = self.external_sample_garbage_receiver.pop() {
            drop(garbage);
        }
    }

    pub fn set_param(&self, key: ParamKey, val: f32) -> Result<(), ParamError> {
        self.param_front_end.set_param(key, val)
    }

    #[inline(always)]
    pub fn get_param(&self, key: ParamKey) -> Result<f32, ParamError> {
        self.param_front_end.get_param(key)
    }

    pub fn get_param_key(&self, name: &'static str) -> Result<ParamKey, ParamError> {
        self.param_front_end.get_key(name)
    }

    pub fn get_all(&self) -> Vec<f32> {
        self.param_front_end.get_all()
    }
}
