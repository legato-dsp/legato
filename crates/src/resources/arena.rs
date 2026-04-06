use crate::resources::window::Window;

// TODO: See if padding with 64 bytes helps cache performance.

/// The non-realtime safe "allocator"
///
/// Really, we have two mental models for allocation:
///
/// - Alloc: normal malloc or whatever implementation, these are resources allocated in the LegatoBuilder lifecycle
/// - RTAlloc: preallocated, and we simply give a [`Window`] that that can slice a chunk of preallocated memory.
///
/// This here is the version used during the LegatoBuilder lifecycle.
#[derive(Clone, Default)]
pub struct Arena {
    data: Vec<f32>,
    cursor: usize,
}

/// The real-time safe "allocator"
///
/// Here, we no longer have a resizable container. If a node needs
/// an underlying block of &[f32] to operate, the memory must already
/// be allocated and owned at this point.
///
/// The amount of realtime memory available is determined in the seal function.
#[derive(Clone, Default)]
pub struct RuntimeArena {
    data: Box<[f32]>,
    scratch_start: usize, // where allocations from "build" time resources cease. The area after here is then usable for realtime "allocation"
    rt_cursor: usize,     // starts at scratch_start, grows into extra region
}

impl Arena {
    pub fn new(capacity_hint: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity_hint),
            cursor: 0,
        }
    }

    pub fn alloc(&mut self, size: usize) -> Window {
        if self.cursor + size > self.data.len() {
            self.data.resize(self.cursor + size, 0.0);
        }
        let start = self.cursor;
        self.cursor += size;
        Window { start, len: size }
    }

    pub fn seal(mut self, rt_capacity: usize) -> RuntimeArena {
        // Here, we append rt_capacity onto our current capacity
        let total = self.cursor + rt_capacity;
        self.data.resize(total, 0.0);

        RuntimeArena {
            data: self.data.into_boxed_slice(),
            scratch_start: self.cursor,
            rt_cursor: self.cursor,
        }
    }
}

impl RuntimeArena {
    /// Allocate a window from the rt region. Intended for
    /// uses like LUTs, wavetable scratch, grain buffers, etc.
    ///
    /// Panics if rt capacity is exceeded.
    pub fn rt_alloc(&mut self, len: usize) -> Window {
        assert!(
            self.rt_cursor + len <= self.data.len(),
            "RuntimeArena Capacity Exceeded!: requested {len}, {} remaining",
            self.data.len() - self.rt_cursor
        );
        let start = self.rt_cursor;
        self.rt_cursor += len;
        Window { start, len }
    }

    /// Reset the rt cursor, making the entire extra region
    /// available again. Call between frames or when scratch
    /// content is no longer needed.
    ///
    /// TODO: Do we zero this out or could that be tough on realtime safety?
    pub fn rt_reset(&mut self) {
        self.rt_cursor = self.scratch_start;
    }

    #[inline(always)]
    pub fn slice(&self, w: Window) -> &[f32] {
        debug_assert!(w.start + w.len <= self.data.len());
        &self.data[w.start..w.start + w.len]
    }

    #[inline(always)]
    pub fn slice_mut(&mut self, w: Window) -> &mut [f32] {
        debug_assert!(w.start + w.len <= self.data.len());
        &mut self.data[w.start..w.start + w.len]
    }

    pub fn rt_allocated(&self) -> usize {
        self.rt_cursor - self.scratch_start
    }

    pub fn rt_capacity(&self) -> usize {
        self.data.len() - self.rt_cursor
    }

    pub fn total_capacity(&self) -> usize {
        self.data.len()
    }
}
