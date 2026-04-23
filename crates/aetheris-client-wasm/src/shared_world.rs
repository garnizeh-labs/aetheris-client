//! Lock-free double-buffered compact replication layout.
//!
//! This module implements the "Shared World" logic which allows the Game Worker
//! to write authoritative updates while the Render Worker reads a stable snapshot
//! without blocking, satisfying the zero-cost synchronization requirement of M360.

use bytemuck::{Pod, Zeroable};
use core::sync::atomic::{AtomicU64, Ordering};
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
    /// Mining state (0: inactive, 1: active).
    pub mining_active: u8, // Offset 43, size 1
    /// Current cargo count.
    pub cargo_ore: u16, // Offset 44, size 2
    /// Network ID of the mining target (truncated to 16-bit for Phase 1).
    pub mining_target_id: u16, // Offset 46, size 2
}

/// The header for the `SharedArrayBuffer`.
///
/// `state` packs `entity_count` (high 32 bits) and `flip_bit` (low 32 bits) into a
/// single `AtomicU64` so that readers always observe a consistent pair with a single
/// acquire load, eliminating the TOCTOU window that existed when they were separate
/// `AtomicU32` fields.
#[derive(Debug)]
#[repr(C)]
pub struct SabHeader {
    /// Packed atomic state: `high 32 bits = entity_count`, `low 32 bits = flip_bit` (0 or 1).
    /// Updated with a single `Release` store in `commit_write`.
    pub state: AtomicU64, // Offset 0
    /// The latest server tick corresponding to the data in the active buffer.
    pub tick: AtomicU64, // Offset 8
    pub room_min_x: core::sync::atomic::AtomicU32, // Offset 16
    pub room_min_y: core::sync::atomic::AtomicU32, // Offset 20
    pub room_max_x: core::sync::atomic::AtomicU32, // Offset 24
    pub room_max_y: core::sync::atomic::AtomicU32, // Offset 28
    /// Seqlock counter for room bounds. Odd = write in progress; even = stable.
    pub room_bounds_seq: core::sync::atomic::AtomicU32, // Offset 32
    /// Sub-tick progress (0.0 to 1.0) for visual interpolation.
    pub sub_tick_fraction: core::sync::atomic::AtomicU32, // Offset 36
}

/// Total size in bytes required for the compact replication layout.
/// 32 bytes (Header) + 384 KiB (Buffer A) + 384 KiB (Buffer B) = 768 KiB + 32 bytes.
/// Note: Rounded to 768 KiB in documentation, exact size is 786,464 bytes.
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
        (self.header().state.load(Ordering::Acquire) & 0xFFFF_FFFF) as u32
    }

    /// Returns the entity count for the active buffer.
    #[must_use]
    pub fn entity_count(&self) -> u32 {
        (self.header().state.load(Ordering::Acquire) >> 32) as u32
    }

    /// Returns the server tick for the active buffer.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.header().tick.load(Ordering::Acquire)
    }

    /// Returns the sub-tick progress fraction (0.0 to 1.0).
    #[must_use]
    pub fn sub_tick_fraction(&self) -> f32 {
        f32::from_bits(self.header().sub_tick_fraction.load(Ordering::Acquire))
    }

    /// Updates the sub-tick progress fraction.
    pub fn set_sub_tick_fraction(&mut self, fraction: f32) {
        self.header()
            .sub_tick_fraction
            .store(fraction.to_bits(), Ordering::Release);
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
    ///
    /// Both the active buffer index and the entity count are derived from a single
    /// atomic load, so readers always see a consistent pair.
    #[must_use]
    pub fn get_read_buffer(&self) -> &[SabSlot] {
        let state = self.header().state.load(Ordering::Acquire);
        let active = (state & 0xFFFF_FFFF) as usize;
        let count = ((state >> 32) as usize).min(MAX_ENTITIES);
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

        let packed = (u64::from(entity_count) << 32) | u64::from(next_active);
        self.header().tick.store(tick, Ordering::Release);
        self.header().state.store(packed, Ordering::Release);
    }

    /// Updates the room bounds using a seqlock so readers always see a consistent
    /// rectangle. The sequence number is bumped to an odd value before writing and
    /// back to an even value (with `Release` ordering) after, matching the acquire
    /// fence in `get_room_bounds`.
    pub fn set_room_bounds(&mut self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) {
        let h = self.header();
        let seq = h.room_bounds_seq.load(Ordering::Relaxed);
        // Mark write in progress: odd sequence number.
        h.room_bounds_seq
            .store(seq.wrapping_add(1), Ordering::Relaxed);
        core::sync::atomic::fence(Ordering::Release);
        h.room_min_x.store(min_x.to_bits(), Ordering::Relaxed);
        h.room_min_y.store(min_y.to_bits(), Ordering::Relaxed);
        h.room_max_x.store(max_x.to_bits(), Ordering::Relaxed);
        h.room_max_y.store(max_y.to_bits(), Ordering::Relaxed);
        // Mark write complete: even sequence number, visible to readers.
        h.room_bounds_seq
            .store(seq.wrapping_add(2), Ordering::Release);
    }

    /// Reads the room bounds, retrying if a concurrent write is detected via the
    /// seqlock. Guaranteed to return a consistent (non-torn) rectangle.
    #[must_use]
    pub fn get_room_bounds(&self) -> (f32, f32, f32, f32) {
        let h = self.header();
        loop {
            let seq1 = h.room_bounds_seq.load(Ordering::Acquire);
            if seq1 & 1 != 0 {
                // Write in progress — spin.
                core::hint::spin_loop();
                continue;
            }
            let min_x = f32::from_bits(h.room_min_x.load(Ordering::Relaxed));
            let min_y = f32::from_bits(h.room_min_y.load(Ordering::Relaxed));
            let max_x = f32::from_bits(h.room_max_x.load(Ordering::Relaxed));
            let max_y = f32::from_bits(h.room_max_y.load(Ordering::Relaxed));
            core::sync::atomic::fence(Ordering::Acquire);
            let seq2 = h.room_bounds_seq.load(Ordering::Relaxed);
            if seq1 == seq2 {
                return (min_x, min_y, max_x, max_y);
            }
            // Torn read — retry.
            core::hint::spin_loop();
        }
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
