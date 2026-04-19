//! Lock-free double-buffered compact replication layout.
//!
//! This module implements the "Shared World" logic which allows the Game Worker
//! to write authoritative updates while the Render Worker reads a stable snapshot
//! without blocking, satisfying the zero-cost synchronization requirement of M360.

use bytemuck::{Pod, Zeroable};
use core::sync::atomic::{AtomicU32, Ordering};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

static VALID_POINTERS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

fn get_registry() -> &'static Mutex<HashSet<usize>> {
    VALID_POINTERS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Maximum number of entities supported in the compact shared buffer.
/// Total slots = 16,384 (8,192 per buffer).
pub const MAX_ENTITIES: usize = 8192;

/// A single replicated entity state in the compact shared buffer (48 bytes).
/// Optimized for Void Rush (2D gameplay with 3D elevation).
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SabSlot {
    /// Network-wide unique entity identifier.
    pub network_id: u64, // Offset 0, size 8
    /// World-space position X.
    pub x: f32, // Offset 8, size 4
    /// World-space position Y.
    pub y: f32, // Offset 12, size 4
    /// World-space position Z.
    pub z: f32, // Offset 16, size 4
    /// Orientation yaw (rotation around Z axis).
    pub rotation: f32, // Offset 20, size 4
    /// Velocity vector X.
    pub dx: f32, // Offset 24, size 4
    /// Velocity vector Y.
    pub dy: f32, // Offset 28, size 4
    /// Velocity vector Z.
    pub dz: f32, // Offset 32, size 4
    /// Current health points.
    pub hp: u16, // Offset 36, size 2
    /// Current shield points.
    pub shield: u16, // Offset 38, size 2
    /// Entity type identifier.
    pub entity_type: u16, // Offset 40, size 2
    /// Bitfield flags (Alive: 0, Visible: 1, `LocalPlayer`: 2, Interpolate: 3, ...).
    pub flags: u8, // Offset 42, size 1
    /// Padding for 48-byte total size and alignment.
    pub padding: [u8; 5], // Offset 43, size 5
}

/// The header for the `SharedArrayBuffer`.
#[derive(Debug)]
#[repr(C)]
pub struct SabHeader {
    /// Atomic flip bit (0 or 1) indicating which buffer is currently ACTIVE for readers.
    pub flip_bit: AtomicU32, // Offset 0
    /// Number of active entities in the visible buffer.
    pub entity_count: AtomicU32, // Offset 4
    /// The latest server tick corresponding to the data in the active buffer.
    pub tick: core::sync::atomic::AtomicU64, // Offset 8
}

/// Total size in bytes required for the compact replication layout.
/// 16 bytes (Header) + 384 KiB (Buffer A) + 384 KiB (Buffer B) = 768 KiB + 16 bytes.
/// Note: Rounded to 768 KiB in documentation, exact size is 786,448 bytes.
pub const SHARED_MEMORY_SIZE: usize =
    core::mem::size_of::<SabHeader>() + (core::mem::size_of::<SabSlot>() * MAX_ENTITIES * 2);

/// Returns the size in bytes required for the shared world buffer.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn shared_world_size() -> usize {
    SHARED_MEMORY_SIZE
}

/// A lock-free double buffer for compact entity replication.
/// This points into a `SharedArrayBuffer` allocated by the Main Thread.
pub struct SharedWorld {
    ptr: *mut u8,
    owns_memory: bool,
}

impl SharedWorld {
    /// Initializes the `SharedWorld` from a raw memory pointer.
    ///
    /// # Safety
    /// The pointer must remain valid for the lifetime of this object and must
    /// point to a region of at least `SHARED_MEMORY_SIZE` bytes.
    pub unsafe fn from_ptr(ptr: *mut u8) -> Self {
        Self {
            ptr,
            owns_memory: false,
        }
    }

    /// Creates a new `SharedWorld` by allocating its own memory (fallback/local use).
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new() -> Self {
        let layout = core::alloc::Layout::from_size_align(SHARED_MEMORY_SIZE, 8)
            .expect("Invalid SHARED_MEMORY_SIZE or alignment constants");
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };

        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        // Register the pointer for JS-boundary validation
        get_registry()
            .lock()
            .expect("Registry mutex poisoned")
            .insert(ptr as usize);

        Self {
            ptr,
            owns_memory: true,
        }
    }

    /// Validates if a raw pointer was registered by a living `SharedWorld` instance.
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn is_valid(ptr: *mut u8) -> bool {
        get_registry()
            .lock()
            .expect("Registry mutex poisoned")
            .contains(&(ptr as usize))
    }

    /// Returns the raw pointer to the base of the shared world buffer.
    #[must_use]
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }

    #[allow(clippy::cast_ptr_alignment)]
    fn header(&self) -> &SabHeader {
        unsafe { &*(self.ptr.cast::<SabHeader>()) }
    }

    /// Returns the active buffer index (0 or 1).
    #[must_use]
    pub fn active_index(&self) -> u32 {
        self.header().flip_bit.load(Ordering::Acquire)
    }

    /// Returns the entity count for the active buffer.
    #[must_use]
    pub fn entity_count(&self) -> u32 {
        self.header().entity_count.load(Ordering::Acquire)
    }

    /// Returns the server tick for the active buffer.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.header().tick.load(Ordering::Acquire)
    }

    /// Returns a slice of the entities in the buffer index i.
    #[allow(clippy::cast_ptr_alignment)]
    fn get_buffer(&self, idx: usize) -> &[SabSlot] {
        let offset = core::mem::size_of::<SabHeader>()
            + (idx * MAX_ENTITIES * core::mem::size_of::<SabSlot>());
        unsafe { core::slice::from_raw_parts(self.ptr.add(offset).cast::<SabSlot>(), MAX_ENTITIES) }
    }

    /// Returns a mutable slice of the entities in the buffer index i.
    #[allow(clippy::cast_ptr_alignment)]
    fn get_buffer_mut(&mut self, idx: usize) -> &mut [SabSlot] {
        let offset = core::mem::size_of::<SabHeader>()
            + (idx * MAX_ENTITIES * core::mem::size_of::<SabSlot>());
        unsafe {
            core::slice::from_raw_parts_mut(self.ptr.add(offset).cast::<SabSlot>(), MAX_ENTITIES)
        }
    }

    /// Returns the entities currently visible to readers.
    #[must_use]
    pub fn get_read_buffer(&self) -> &[SabSlot] {
        let active = self.active_index() as usize;
        let count = self.entity_count() as usize;
        &self.get_buffer(active)[..count]
    }

    /// Returns the buffer currently available for writing (inactive buffer).
    #[must_use]
    pub fn get_write_buffer(&mut self) -> &mut [SabSlot] {
        let active = self.active_index() as usize;
        let inactive = 1 - active;
        self.get_buffer_mut(inactive)
    }

    /// Swaps the active buffer and updates the entity count and tick.
    pub fn commit_write(&mut self, entity_count: u32, tick: u64) {
        let active = self.active_index();
        let next_active = 1 - active;

        // Ordering::Release ensures all writes to the buffer are visible
        // before the flip bit or count changes.
        self.header().tick.store(tick, Ordering::Release);
        self.header()
            .entity_count
            .store(entity_count, Ordering::Release);
        self.header().flip_bit.store(next_active, Ordering::Release);
    }
}

impl Drop for SharedWorld {
    #[allow(clippy::missing_panics_doc)]
    fn drop(&mut self) {
        if self.owns_memory {
            if let Ok(mut reg) = get_registry().lock() {
                reg.remove(&(self.ptr as usize));
            }

            let layout = core::alloc::Layout::from_size_align(SHARED_MEMORY_SIZE, 8)
                .expect("Invalid SHARED_MEMORY_SIZE or alignment constants");

            unsafe { std::alloc::dealloc(self.ptr, layout) };
        }
    }
}

impl Default for SharedWorld {
    fn default() -> Self {
        Self::new()
    }
}
