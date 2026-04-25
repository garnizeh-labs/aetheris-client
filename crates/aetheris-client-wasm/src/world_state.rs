//! Client-side world state and interpolation buffer.
//!
//! This module implements the `WorldState` trait for the WASM client,
//! providing the foundation for the two-tick interpolation buffer.

use crate::shared_world::SabSlot;
use aetheris_protocol::error::WorldError;
use aetheris_protocol::events::{ComponentUpdate, ReplicationEvent};
use aetheris_protocol::traits::WorldState;
use aetheris_protocol::types::{
    ClientId, ComponentKind, LocalId, NetworkId, ShipClass, ShipStats, Transform,
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug)]
pub struct InputRecord {
    pub tick: u64,
    pub move_x: f32,
    pub move_y: f32,
    pub actions_mask: u8,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, Default)]
pub struct Velocity {
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
}

/// A simplified client-side world that tracks entity states using `SabSlot`.
#[derive(Debug)]
pub struct ClientWorld {
    /// Map of `NetworkId` to the last known authoritative state.
    pub entities: BTreeMap<NetworkId, SabSlot>,
    /// The network ID of the player's own ship, if known.
    pub player_network_id: Option<NetworkId>,
    /// The latest tick received from the server.
    pub latest_tick: u64,
    /// Manifest of system-wide metadata.
    pub system_manifest: BTreeMap<String, String>,
    /// Optional shared world to push room bounds into directly (stored as usize for Send/Sync).
    pub shared_world_ref: Option<usize>,
    /// History of inputs applied for Client-Side Prediction reconciliation.
    pub input_history: VecDeque<InputRecord>,
    /// The latest server tick that has been reconciled.
    pub last_reconciled_tick: u64,
    /// When `true`, the client simulates input locally and replays unacknowledged inputs
    /// on top of server snapshots (client-side prediction + reconciliation).
    /// When `false`, the client is pure server-authority: position is only updated from
    /// server transforms and no local simulation is performed for the local player.
    pub prediction_enabled: bool,
    /// Authoritative world boundaries received from the server.
    /// Used for toroidal wrapping in Sandbox mode.
    pub room_bounds: Option<aetheris_protocol::types::RoomBounds>,
}

impl Default for ClientWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientWorld {
    /// Creates a new, empty `ClientWorld`.
    ///
    /// `prediction_enabled` controls whether client-side prediction is active.
    /// Pass `false` for pure server-authority mode (recommended during debugging).
    /// Pass `true` to enable local simulation + reconciliation for responsive feel.
    #[must_use]
    pub fn new() -> Self {
        Self::with_prediction(false)
    }

    /// Creates a `ClientWorld` with explicit prediction setting.
    #[must_use]
    pub fn with_prediction(prediction_enabled: bool) -> Self {
        Self {
            entities: BTreeMap::new(),
            player_network_id: None,
            latest_tick: 0,
            system_manifest: BTreeMap::new(),
            shared_world_ref: None,
            input_history: VecDeque::with_capacity(120), // 2 seconds at 60Hz
            last_reconciled_tick: 0,
            prediction_enabled,
            room_bounds: if prediction_enabled {
                Some(aetheris_protocol::types::RoomBounds {
                    min_x: -250.0,
                    min_y: -250.0,
                    max_x: 250.0,
                    max_y: 250.0,
                })
            } else {
                None
            },
        }
    }
}

impl WorldState for ClientWorld {
    fn get_local_id(&self, network_id: NetworkId) -> Option<LocalId> {
        Some(LocalId(network_id.0))
    }

    fn get_network_id(&self, local_id: LocalId) -> Option<NetworkId> {
        Some(NetworkId(local_id.0))
    }

    fn extract_deltas(&mut self) -> Vec<ReplicationEvent> {
        Vec::new()
    }

    fn apply_updates(&mut self, updates: &[(ClientId, ComponentUpdate)]) {
        if !updates.is_empty() {
            tracing::debug!(
                count = updates.len(),
                player_network_id = ?self.player_network_id,
                total_entities = self.entities.len(),
                "[apply_updates] Processing updates batch"
            );
        }
        for (_, update) in updates {
            if update.tick > self.latest_tick {
                self.latest_tick = update.tick;
            }

            let is_new = !self.entities.contains_key(&update.network_id);

            // Ensure entity exists for component updates
            let entry = self.entities.entry(update.network_id).or_insert_with(|| {
                tracing::trace!(
                    network_id = update.network_id.0,
                    kind = update.component_kind.0,
                    player_network_id = ?self.player_network_id,
                    "[apply_updates] NEW entity from server"
                );
                SabSlot {
                    network_id: update.network_id.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    rotation: 0.0,
                    dx: 0.0,
                    dy: 0.0,
                    dz: 0.0,
                    hp: 100,
                    shield: 0,
                    entity_type: 0,
                    flags: 1,
                    cargo_ore: 0,
                    cargo_capacity: 0,
                    mining_target_id: 0,
                    mining_active: 0,
                    padding: [0; 6],
                }
            });

            // Re-apply possession flag if this is the player ship.
            // This handles cases where Possession arrived before the entity was spawned.
            let is_player = Some(update.network_id) == self.player_network_id;
            if is_player {
                tracing::trace!(
                    network_id = update.network_id.0,
                    is_new,
                    "[apply_updates] Setting 0x04 (LocalPlayer) flag on entity"
                );
                entry.flags |= 0x04;
            } else if is_new {
                tracing::trace!(
                    network_id = update.network_id.0,
                    player_network_id = ?self.player_network_id,
                    flags = entry.flags,
                    "[apply_updates] New NON-player entity - no possession flag"
                );
            }

            self.apply_component_update(update);
        }
    }

    fn simulate(&mut self) {
        const DRAG: f32 = 1.0;
        const DT: f32 = 1.0 / 60.0;
        let drag_factor = 1.0 / (1.0 + DRAG * DT);

        for slot in self.entities.values_mut() {
            // Semi-implicit Euler integration
            slot.dx *= drag_factor;
            slot.dy *= drag_factor;
            slot.x += slot.dx * DT;
            slot.y += slot.dy * DT;

            // Toroidal wrapping (Sandbox/Prediction)
            if let Some(bounds) = self.room_bounds {
                let width = bounds.max_x - bounds.min_x;
                let height = bounds.max_y - bounds.min_y;
                if width > 0.0 {
                    slot.x = ((slot.x - bounds.min_x).rem_euclid(width)) + bounds.min_x;
                }
                if height > 0.0 {
                    slot.y = ((slot.y - bounds.min_y).rem_euclid(height)) + bounds.min_y;
                }
            }
        }
    }

    fn spawn_networked(&mut self) -> NetworkId {
        NetworkId(0)
    }

    fn spawn_networked_for(&mut self, _client_id: ClientId) -> NetworkId {
        self.spawn_networked()
    }

    fn despawn_networked(&mut self, network_id: NetworkId) -> Result<(), WorldError> {
        self.entities
            .remove(&network_id)
            .map(|_| ())
            .ok_or(WorldError::EntityNotFound(network_id))
    }

    fn stress_test(&mut self, _count: u16, _rotate: bool) {}

    fn spawn_kind(&mut self, _kind: u16, _x: f32, _y: f32, _rot: f32) -> NetworkId {
        NetworkId(1)
    }

    fn clear_world(&mut self) {
        self.entities.clear();
    }

    fn state_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        use twox_hash::XxHash64;

        // Use a stable, seeded hasher for cross-platform determinism
        let mut hasher = XxHash64::with_seed(0);
        self.latest_tick.hash(&mut hasher);

        // BTreeMap iteration is already deterministic (sorted by NetworkId)
        for (nid, slot) in &self.entities {
            nid.hash(&mut hasher);

            // SabSlot fields must be hashed individually as the struct is FFI-oriented
            slot.x.to_bits().hash(&mut hasher);
            slot.y.to_bits().hash(&mut hasher);
            slot.z.to_bits().hash(&mut hasher);
            slot.rotation.to_bits().hash(&mut hasher);

            // Inclusion of physics and mining state for high-fidelity determinism (VS-07 §4.2)
            slot.dx.to_bits().hash(&mut hasher);
            slot.dy.to_bits().hash(&mut hasher);
            slot.dz.to_bits().hash(&mut hasher);

            slot.hp.hash(&mut hasher);
            slot.shield.hash(&mut hasher);
            slot.entity_type.hash(&mut hasher);
            slot.flags.hash(&mut hasher);

            slot.mining_active.hash(&mut hasher);
            slot.cargo_ore.hash(&mut hasher);
            slot.mining_target_id.hash(&mut hasher);
        }

        hasher.finish()
    }
}

impl ClientWorld {
    /// Handles discrete game events from the server.
    pub fn handle_game_event(&mut self, event: &aetheris_protocol::events::GameEvent) {
        if let aetheris_protocol::events::GameEvent::Possession { network_id } = event {
            let prev = self.player_network_id;
            tracing::info!(
                ?network_id,
                ?prev,
                entity_exists = self.entities.contains_key(network_id),
                total_entities = self.entities.len(),
                "[handle_game_event] POSSESSION received — updating player_network_id"
            );
            // Clear the local-player flag from the previous entity if it differs.
            if let Some(slot) = prev
                .filter(|&id| id != *network_id)
                .and_then(|id| self.entities.get_mut(&id))
            {
                slot.flags &= !0x04;
                tracing::info!(
                    network_id = ?prev,
                    flags = slot.flags,
                    "[handle_game_event] 0x04 flag cleared from previous entity"
                );
            }
            self.player_network_id = Some(*network_id);
            if let Some(slot) = self.entities.get_mut(network_id) {
                slot.flags |= 0x04;
                tracing::info!(
                    ?network_id,
                    flags = slot.flags,
                    "[handle_game_event] 0x04 flag applied to entity"
                );
            } else {
                tracing::warn!(
                    ?network_id,
                    "[handle_game_event] Possession entity not yet in world - will apply when it arrives"
                );
            }
        } else if let aetheris_protocol::events::GameEvent::DamageEvent { target, amount } = event {
            tracing::info!(?target, amount, "[handle_game_event] DamageEvent received");
            // In a real client, this would trigger a screen shake or UI flash.
            // For now, we rely on the component update to update the HP/Shield.
        } else if let aetheris_protocol::events::GameEvent::DeathEvent { target } = event {
            tracing::info!(?target, "[handle_game_event] DeathEvent received");
            // Despawn will happen when the server stops replicating the entity,
            // but we can mark it as dead or play an explosion VFX here.
        } else if let aetheris_protocol::events::GameEvent::RespawnEvent { target, x, y } = event {
            tracing::info!(?target, x, y, "[handle_game_event] RespawnEvent received");
        }
    }

    fn apply_component_update(&mut self, update: &ComponentUpdate) {
        match update.component_kind {
            ComponentKind(1) => self.handle_transform_update(update),
            ComponentKind(2) => self.handle_velocity_update(update),
            ComponentKind(5) => self.handle_ship_class_update(update),
            ComponentKind(3) => self.handle_ship_stats_update(update),
            aetheris_protocol::types::MINING_BEAM_KIND => self.handle_mining_beam_update(update),
            aetheris_protocol::types::CARGO_HOLD_KIND => self.handle_cargo_hold_update(update),
            aetheris_protocol::types::ASTEROID_KIND => {
                if let Some(entry) = self.entities.get_mut(&update.network_id)
                    && entry.entity_type == 0
                {
                    entry.entity_type = 5;
                }
            }
            aetheris_protocol::types::SHIELD_POOL_KIND => self.handle_shield_pool_update(update),
            aetheris_protocol::types::HULL_POOL_KIND => self.handle_hull_pool_update(update),
            aetheris_protocol::types::CARGO_DROP_KIND => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.entity_type = 6;
                }
            }
            aetheris_protocol::types::ROOM_BOUNDS_KIND => self.handle_room_bounds_update(update),
            kind => {
                tracing::debug!(
                    network_id = update.network_id.0,
                    kind = kind.0,
                    "Unhandled component kind"
                );
            }
        }
    }

    fn handle_transform_update(&mut self, update: &ComponentUpdate) {
        match rmp_serde::from_slice::<Transform>(&update.payload) {
            Ok(transform) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    if (entry.flags & 0x04) != 0 && self.prediction_enabled {
                        // M1020: Client-Side Prediction + Reconciliation
                        let mut authoritative_x = transform.x;
                        let mut authoritative_y = transform.y;

                        // M1038: Wrap-aware snap
                        if let Some(bounds) = self.room_bounds {
                            let width = bounds.max_x - bounds.min_x;
                            let height = bounds.max_y - bounds.min_y;
                            if width > 0.0 {
                                let dx = authoritative_x - entry.x;
                                if dx.abs() > width * 0.5 {
                                    if dx > 0.0 {
                                        authoritative_x -= width;
                                    } else {
                                        authoritative_x += width;
                                    }
                                }
                            }
                            if height > 0.0 {
                                let dy = authoritative_y - entry.y;
                                if dy.abs() > height * 0.5 {
                                    if dy > 0.0 {
                                        authoritative_y -= height;
                                    } else {
                                        authoritative_y += height;
                                    }
                                }
                            }
                        }

                        entry.x = authoritative_x;
                        entry.y = authoritative_y;
                        entry.z = transform.z;
                        entry.rotation = transform.rotation;

                        let server_tick = update.tick;
                        for record in self.input_history.iter().filter(|r| r.tick > server_tick) {
                            Self::simulate_slot_wrapped(
                                entry,
                                record.move_x,
                                record.move_y,
                                self.room_bounds,
                            );
                        }
                        while self
                            .input_history
                            .front()
                            .is_some_and(|r| r.tick <= server_tick)
                        {
                            self.input_history.pop_front();
                        }
                    } else {
                        // Server-authority mode
                        entry.x = transform.x;
                        entry.y = transform.y;
                        entry.z = transform.z;
                        entry.rotation = transform.rotation;
                        self.input_history.clear();
                    }

                    if transform.entity_type != 0 {
                        entry.entity_type = transform.entity_type;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode Transform");
            }
        }
    }

    fn handle_velocity_update(&mut self, update: &ComponentUpdate) {
        match rmp_serde::from_slice::<Velocity>(&update.payload) {
            Ok(velocity) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.dx = velocity.dx;
                    entry.dy = velocity.dy;
                    entry.dz = velocity.dz;
                }
            }
            Err(e) => {
                tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode Velocity");
            }
        }
    }

    fn handle_ship_class_update(&mut self, update: &ComponentUpdate) {
        match rmp_serde::from_slice::<ShipClass>(&update.payload) {
            Ok(ship_class) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.entity_type = match ship_class {
                        ShipClass::Interceptor => 1,
                        ShipClass::Dreadnought => 3,
                        ShipClass::Hauler => 4,
                    };
                }
            }
            Err(e) => {
                tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode ShipClass");
            }
        }
    }

    fn handle_ship_stats_update(&mut self, update: &ComponentUpdate) {
        match rmp_serde::from_slice::<ShipStats>(&update.payload) {
            Ok(stats) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.hp = stats.hp;
                    entry.shield = stats.shield;
                }
            }
            Err(e) => {
                tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode ShipStats");
            }
        }
    }

    fn handle_mining_beam_update(&mut self, update: &ComponentUpdate) {
        use aetheris_protocol::types::MiningBeam;
        match rmp_serde::from_slice::<MiningBeam>(&update.payload) {
            Ok(beam) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.mining_active = u8::from(beam.active);
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        entry.mining_target_id = beam.target.map_or(0, |id| id.0 as u16);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    network_id = update.network_id.0,
                    error = ?e,
                    payload = %hex::encode(&update.payload),
                    "Failed to decode MiningBeam"
                );
            }
        }
    }

    fn handle_cargo_hold_update(&mut self, update: &ComponentUpdate) {
        use aetheris_protocol::types::CargoHold;
        match rmp_serde::from_slice::<CargoHold>(&update.payload) {
            Ok(cargo) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.cargo_ore = cargo.ore_count;
                    entry.cargo_capacity = cargo.capacity;
                    entry.flags |= 0x04;
                }
            }
            Err(e) => {
                tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode CargoHold");
            }
        }
    }

    fn handle_room_bounds_update(&mut self, update: &ComponentUpdate) {
        use aetheris_protocol::types::RoomBounds;
        if let (Ok(bounds), Some(ptr_val)) = (
            rmp_serde::from_slice::<RoomBounds>(&update.payload),
            self.shared_world_ref,
        ) {
            let mut sw = unsafe { crate::shared_world::SharedWorld::from_ptr(ptr_val as *mut u8) };
            sw.set_room_bounds(bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y);
            self.room_bounds = Some(bounds);
        }
    }

    fn handle_shield_pool_update(&mut self, update: &ComponentUpdate) {
        use aetheris_protocol::types::ShieldPool;
        match rmp_serde::from_slice::<ShieldPool>(&update.payload) {
            Ok(pool) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.shield = pool.current;
                }
            }
            Err(e) => {
                tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode ShieldPool");
            }
        }
    }

    fn handle_hull_pool_update(&mut self, update: &ComponentUpdate) {
        use aetheris_protocol::types::HullPool;
        match rmp_serde::from_slice::<HullPool>(&update.payload) {
            Ok(pool) => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    entry.hp = pool.current;
                }
            }
            Err(e) => {
                tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode HullPool");
            }
        }
    }
}

impl ClientWorld {
    /// Internal simulation step used for prediction and reconciliation.
    fn simulate_slot(slot: &mut SabSlot, move_x: f32, move_y: f32) {
        const THRUST_FORCE: f32 = 8000.0;
        const BASE_MASS: f32 = 100.0;
        const MASS_PER_ORE: f32 = 2.0;
        const DRAG: f32 = 2.0;
        const MAX_SPEED: f32 = 75.0;
        const DT: f32 = 1.0 / 60.0;

        // 1.0. Calculate total mass (M1038 Cargo Penalty)
        let total_mass = BASE_MASS + (f32::from(slot.cargo_ore) * MASS_PER_ORE);

        // 1. Resolve move vector (Normalizing diagonal)
        let mut mx = move_x;
        let mut my = move_y;
        let input_len_sq = mx * mx + my * my;
        if input_len_sq > 1.0 {
            let input_len = input_len_sq.sqrt();
            mx /= input_len;
            my /= input_len;
        }

        // 2. Apply thrust acceleration
        let accel_x = mx * (THRUST_FORCE / total_mass);
        let accel_y = my * (THRUST_FORCE / total_mass);

        slot.dx += accel_x * DT;
        slot.dy += accel_y * DT;

        // 3. Apply Drag (Stable semi-implicit model)
        let drag_factor = 1.0 / (1.0 + DRAG * DT);
        slot.dx *= drag_factor;
        slot.dy *= drag_factor;

        // 4. Clamp speed
        let speed_sq = slot.dx * slot.dx + slot.dy * slot.dy;
        if speed_sq > MAX_SPEED * MAX_SPEED {
            let speed = speed_sq.sqrt();
            slot.dx = (slot.dx / speed) * MAX_SPEED;
            slot.dy = (slot.dy / speed) * MAX_SPEED;
        }

        // 5. Update rotation (Smoothing)
        if speed_sq > 0.01 {
            const TURN_RATE: f32 = 5.0;
            let target_rot = slot.dy.atan2(slot.dx);
            let current_rot = slot.rotation;
            let diff = (target_rot - current_rot + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;

            if diff.abs() > 0.001 {
                slot.rotation += diff.clamp(-TURN_RATE * DT, TURN_RATE * DT);
            } else {
                slot.rotation = target_rot;
            }
        }

        // 6. Integrate position
        slot.x += slot.dx * DT;
        slot.y += slot.dy * DT;
        slot.z += slot.dz * DT;
    }

    /// Internal simulation step with toroidal wrapping.
    ///
    /// NOTE: If prediction is enabled, wrapping must be consistent with the server
    /// to avoid reconciliation snaps. Prediction is currently disabled in the playground.
    fn simulate_slot_wrapped(
        slot: &mut SabSlot,
        move_x: f32,
        move_y: f32,
        bounds: Option<aetheris_protocol::types::RoomBounds>,
    ) {
        Self::simulate_slot(slot, move_x, move_y);

        if let Some(bounds) = bounds {
            let width = bounds.max_x - bounds.min_x;
            let height = bounds.max_y - bounds.min_y;
            if width > 0.0 {
                slot.x = ((slot.x - bounds.min_x).rem_euclid(width)) + bounds.min_x;
            }
            if height > 0.0 {
                slot.y = ((slot.y - bounds.min_y).rem_euclid(height)) + bounds.min_y;
            }
        }
    }

    /// Applies playground input to the local player entity.
    /// Used for Sandbox mode simulation.
    pub fn playground_apply_input(&mut self, move_x: f32, move_y: f32, actions_mask: u32) -> bool {
        if self.prediction_enabled {
            // Record input for reconciliation history only when prediction is active.
            self.input_history.push_back(InputRecord {
                tick: self.latest_tick,
                move_x,
                move_y,
                #[allow(clippy::cast_possible_truncation)]
                actions_mask: actions_mask as u8,
            });

            // Limit history size to 5 seconds (300 ticks)
            if self.input_history.len() > 300 {
                self.input_history.pop_front();
            }
        }

        let mut found = false;
        // Locate local player entity (flag 0x04) in the world state
        for slot in self.entities.values_mut() {
            if (slot.flags & 0x04) != 0 {
                found = true;
                if self.prediction_enabled {
                    // Prediction ON: simulate locally for immediate visual feedback.
                    // The server reconciles this in apply_component_update().
                    Self::simulate_slot_wrapped(slot, move_x, move_y, self.room_bounds);
                }
                // Prediction OFF: input is only sent to the server.
                // Position is updated exclusively by server transforms when they arrive.
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_world::SabSlot;
    use aetheris_protocol::types::NetworkId;
    use bytemuck::Zeroable;

    #[test]
    fn test_playground_movement() {
        let mut world = ClientWorld::with_prediction(true);

        // Spawn a local player ship (flag 0x04)
        world.entities.insert(
            NetworkId(1),
            SabSlot {
                network_id: 1,
                flags: 0x04, // Local player
                ..SabSlot::zeroed()
            },
        );

        // Apply thrust forward (move_x = 1.0)
        world.playground_apply_input(1.0, 0.0, 0);

        let player = world.entities.get(&NetworkId(1)).unwrap();
        assert!(player.dx > 0.0);
        assert!(player.x > 0.0);
        // accel = 80, DT = 1/60, drag_factor = 1 / (1 + 2.0/60) = 60/62 ≈ 0.9677
        // dx = (0 + 80 * 1/60) * 0.9677 = 1.2903225
        assert!((player.dx - 1.2903225).abs() < 0.0001);
    }

    #[test]
    fn test_playground_speed_clamp() {
        let mut world = ClientWorld::with_prediction(true);
        world.entities.insert(
            NetworkId(1),
            SabSlot {
                network_id: 1,
                flags: 0x04,
                dx: 10.0,
                dy: 10.0,
                ..SabSlot::zeroed()
            },
        );

        // Apply thrust
        world.playground_apply_input(1.0, 1.0, 0);

        let player = world.entities.get(&NetworkId(1)).unwrap();
        let speed = (player.dx * player.dx + player.dy * player.dy).sqrt();
        assert!(speed <= 30.0 + 0.0001);
    }

    #[test]
    fn test_playground_drag() {
        let mut world = ClientWorld::with_prediction(true);
        world.entities.insert(
            NetworkId(1),
            SabSlot {
                network_id: 1,
                flags: 0x04,
                ..SabSlot::zeroed()
            },
        );

        // Start moving
        world.playground_apply_input(1.0, 0.0, 0);
        let v1 = world.entities.get(&NetworkId(1)).unwrap().dx;

        // Coast (zero input)
        world.playground_apply_input(0.0, 0.0, 0);
        let v2 = world.entities.get(&NetworkId(1)).unwrap().dx;

        assert!(v2 < v1);
        // drag_factor = 1 / (1 + 2.0/60) = 60/62 ≈ 0.9677
        assert!((v2 - v1 * (1.0 / (1.0 + 2.0 / 60.0))).abs() < 0.0001);
    }
}
