use ringbuf::{
    Cons, HeapCons, HeapProd, HeapRb, Prod,
    traits::{Consumer, Producer, Split},
};
use slotmap::SlotMap;

use crate::{
    params::{ParamError, ParamKey, ParamMeta, ParamStoreBuilder, ParamStoreFrontend},
    resources_two::{
        DelayLineKey, ExternalBufferKey, ExternalBufferUpdate, InternalBufferKey, Resources,
        arena::Arena, buffer::ExternalBuffer, delay::ResourceDelay, window::Window,
    },
};

#[derive(Clone, Default)]
pub struct ResourceBuilder {
    arena: Arena,
    /// Maps to a Window (used to create a slice) on our "arena"
    internal_buffers: SlotMap<InternalBufferKey, Window>,
    /// Maps to an optional external buffer
    external_buffers: SlotMap<ExternalBufferKey, Option<ExternalBuffer>>,
    /// Underlying hoisted DelayLine that is then constructed into a DelayLineView
    delay_lines: SlotMap<DelayLineKey, ResourceDelay>,
    param_builder: ParamStoreBuilder,
}

impl ResourceBuilder {
    pub fn add_delay_line(&mut self, delay_line: ResourceDelay) -> DelayLineKey {
        self.delay_lines.insert(delay_line)
    }

    pub fn replace_delay_line(&mut self, key: DelayLineKey, delay_line: ResourceDelay) {
        *self.delay_lines.get_mut(key).expect("Delay line not found") = delay_line;
    }

    /// Register an external buffer, and add the window to the resource manager.
    pub fn add_external_buffer(&mut self, buffer: ExternalBuffer) -> ExternalBufferKey {
        self.external_buffers.insert(Some(buffer))
    }

    /// Register an internal buffer of a certain size, and add the window to the resource manager.
    pub fn add_internal_buffer(&mut self, size: usize) -> InternalBufferKey {
        let window = self.arena.alloc(size);
        self.internal_buffers.insert(window)
    }

    pub fn add_param(&mut self, unique_name: String, meta: ParamMeta) -> ParamKey {
        self.param_builder.add_param(unique_name, meta)
    }

    pub fn build(self, rt_capacity: usize) -> (ResourceFrontend, Resources) {
        let (param_frontend, store) = self.param_builder.build();
        // Seal the arena so we can no longer allocate. rt_capacity is the extra space alloted for RT allocation.
        let arena = self.arena.seal(rt_capacity);

        // TODO: Config, is this a sensible default?
        let (garbage_prod, garbage_cons) = HeapRb::new(512).split();

        let (extern_buffer_update_prod, extern_buffer_update_cons) = HeapRb::new(512).split();

        let resources = Resources::new(arena, store, extern_buffer_update_cons, garbage_prod);

        let frontend =
            ResourceFrontend::new(param_frontend, extern_buffer_update_prod, garbage_cons);

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
}

impl ResourceFrontend {
    pub fn new(
        param_front_end: ParamStoreFrontend,
        external_sample_producer: HeapProd<ExternalBufferUpdate>,
        external_sample_garbage_receiver: HeapCons<ExternalBuffer>,
    ) -> Self {
        Self {
            param_front_end,
            external_sample_producer,
            external_sample_garbage_receiver,
        }
    }

    // --------------------------------------------------------
    // Sample Logic
    // --------------------------------------------------------

    /// Send an external buffer to the runtime
    pub fn send_external_buffer(
        &mut self,
        item: ExternalBufferUpdate,
    ) -> Result<(), ExternalBufferUpdate> {
        self.external_sample_producer.try_push(item)
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

    pub fn get_key(&self, name: &'static str) -> Result<ParamKey, ParamError> {
        self.param_front_end.get_key(name)
    }

    pub fn get_all(&self) -> Vec<f32> {
        self.param_front_end.get_all()
    }
}
