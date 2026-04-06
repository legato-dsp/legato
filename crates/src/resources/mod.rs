use std::collections::HashMap;

use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Producer, Split},
};
use slotmap::{SlotMap, new_key_type};

use crate::resources::{
    arena::{Arena, RuntimeArena},
    buffer::ExternalBuffer,
    delay::{DelayLineView, DelayLineViewMut, ResourceDelay},
    params::{ParamError, ParamKey, ParamMeta, ParamStore, ParamStoreBuilder, ParamStoreFrontend},
    window::Window,
};

pub mod arena;
pub mod buffer;
pub mod delay;
pub mod params;
pub mod window;

new_key_type! { pub struct InternalBufferKey; }
new_key_type! { pub struct ExternalBufferKey; }
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
    receiver: ringbuf::HeapCons<ExternalBufferUpdate>,
    garbage_sender: ringbuf::HeapProd<ExternalBuffer>,
}

impl Resources {
    pub fn new(
        arena: RuntimeArena,
        param_store: ParamStore,
        receiver: HeapCons<ExternalBufferUpdate>,
        garbage_sender: HeapProd<ExternalBuffer>,
        internal_buffers: SlotMap<InternalBufferKey, Window>,
        external_buffers: SlotMap<ExternalBufferKey, Option<ExternalBuffer>>,
        delay_lines: SlotMap<DelayLineKey, ResourceDelay>,
    ) -> Self {
        Self {
            arena,
            param_store,
            internal_buffers,
            external_buffers,
            delay_lines,
            receiver,
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

    pub fn drain(&mut self) {
        while let Some(incoming) = self.receiver.try_pop() {
            if let Some(buffer_ref) = self.external_buffers.get_mut(incoming.key) {
                // This returns the old value, and we can then clean it up in another thread
                let old_buffer = Option::replace(buffer_ref, incoming.buffer);
                if let Some(inner) = old_buffer
                    && self.garbage_sender.try_push(inner).is_err()
                {
                    panic!("Returned buffer was not sent to another thread to be dropped!")
                }
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct ResourceBuilder {
    arena: Arena,
    /// Maps to a Window (used to create a slice) on our "arena"
    internal_buffers: SlotMap<InternalBufferKey, Window>,
    /// Maps to an optional external buffer
    external_buffers: SlotMap<ExternalBufferKey, Option<ExternalBuffer>>,
    /// Underlying hoisted DelayLine that is then constructed into a DelayLineView
    delay_lines: SlotMap<DelayLineKey, ResourceDelay>,
    external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
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

    pub fn build(
        self,
        rt_capacity: usize,
        external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
    ) -> (ResourceFrontend, Resources) {
        let (param_frontend, store) = self.param_builder.build();
        // Seal the arena so we can no longer allocate. rt_capacity is the extra space alloted for RT allocation.
        let arena = self.arena.seal(rt_capacity);

        // TODO: Config, is this a sensible default?
        let (garbage_prod, garbage_cons) = HeapRb::new(512).split();

        let (extern_buffer_update_prod, extern_buffer_update_cons) = HeapRb::new(512).split();

        let resources = Resources::new(
            arena,
            store,
            extern_buffer_update_cons,
            garbage_prod,
            self.internal_buffers,
            self.external_buffers,
            self.delay_lines,
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
    external_sample_producer: HeapProd<ExternalBufferUpdate>,
    /// Receive all of the [`ExternalBuffer`]'s that are then dropped on a non-realtime thread.
    external_sample_garbage_receiver: HeapCons<ExternalBuffer>,
    external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
}

impl ResourceFrontend {
    pub fn new(
        param_front_end: ParamStoreFrontend,
        external_sample_producer: HeapProd<ExternalBufferUpdate>,
        external_sample_garbage_receiver: HeapCons<ExternalBuffer>,
        external_buffer_key_lookup: HashMap<String, ExternalBufferKey>,
    ) -> Self {
        Self {
            param_front_end,
            external_sample_producer,
            external_sample_garbage_receiver,
            external_buffer_key_lookup,
        }
    }

    // --------------------------------------------------------
    // Sample Logic
    // --------------------------------------------------------

    /// Send an external buffer to the runtime
    pub fn send_external_buffer(
        &mut self,
        name: &str,
        buffer: ExternalBuffer,
    ) -> Result<(), ExternalBufferUpdate> {
        dbg!(&self.external_buffer_key_lookup);
        let key = self
            .external_buffer_key_lookup
            .get(name)
            .unwrap_or_else(|| panic!("External buffer name {} not found!", name));

        let update = ExternalBufferUpdate { key: *key, buffer };
        self.external_sample_producer.try_push(update)
    }

    /// Garbage collect any external buffers that are no longer in use
    ///
    /// We are doing this here, and not on the audio thread to avoid any non-realtime ops
    pub fn drain_garbage(&mut self) {
        while let Some(garbage) = self.external_sample_garbage_receiver.try_pop() {
            drop(garbage);
        }
    }

    // --------------------------------------------------------
    // Parameter Logic
    // --------------------------------------------------------

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
