use ringbuf::{
    HeapCons, HeapProd,
    traits::{Consumer, Producer},
};
use slotmap::{SlotMap, new_key_type};

use crate::{
    params::{ParamError, ParamKey, ParamStore},
    resources_two::{
        arena::RuntimeArena,
        buffer::ExternalBuffer,
        delay::{DelayLineView, DelayLineViewMut, ResourceDelay},
        window::Window,
    },
};

pub mod arena;
pub mod buffer;
pub mod builder;
pub mod delay;
pub mod window;

new_key_type! { pub struct InternalBufferKey; }
new_key_type! { pub struct ExternalBufferKey; }
new_key_type! { pub struct DelayLineKey; }

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
/// slice of the [`RuntimeArena`], provided we have already allocated this and still have room
/// on the RuntimeArena
///
/// - external resources: Loaded on another thread, and send with  
pub struct Resources {
    arena: RuntimeArena,
    param_store: ParamStore,
    internal_buffers: SlotMap<InternalBufferKey, Window>,
    external_buffers: SlotMap<ExternalBufferKey, ExternalBuffer>,
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
    ) -> Self {
        Self {
            arena,
            param_store,
            internal_buffers: SlotMap::default(),
            external_buffers: SlotMap::default(),
            delay_lines: SlotMap::default(),
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
            Some(w) => Some(&self.arena.slice(*w)),
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
        self.external_buffers.get(key)
    }

    pub fn get_external_buffer_mut(
        &mut self,
        key: ExternalBufferKey,
    ) -> Option<&mut ExternalBuffer> {
        self.external_buffers.get_mut(key)
    }

    #[inline(always)]
    pub fn get_param(&self, param_key: &ParamKey) -> Result<f32, ParamError> {
        self.param_store.get(param_key)
    }

    pub fn drain(&mut self) {
        while let Some(incoming) = self.receiver.try_pop() {
            if let Some(buffer_ref) = self.external_buffers.get_mut(incoming.key) {
                // This returns the old value, and we can then clean it up in another thread
                let old_buffer = std::mem::replace(buffer_ref, incoming.buffer);
                if let Err(_) = self.garbage_sender.try_push(old_buffer) {
                    panic!("Returned buffer was not sent to another thread to be dropped!")
                }
            }
        }
    }
}
