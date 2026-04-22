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
use std::collections::BTreeMap;

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
}

impl Default for ClientWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientWorld {
    /// Creates a new, empty `ClientWorld`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            player_network_id: None,
            latest_tick: 0,
            system_manifest: BTreeMap::new(),
            shared_world_ref: None,
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
        tracing::trace!(
            count = updates.len(),
            player_network_id = ?self.player_network_id,
            total_entities = self.entities.len(),
            "[apply_updates] Processing updates"
        );
        for (_, update) in updates {
            if update.tick > self.latest_tick {
                self.latest_tick = update.tick;
            }

            let is_new = !self.entities.contains_key(&update.network_id);

            // Ensure entity exists for component updates
            let entry = self.entities.entry(update.network_id).or_insert_with(|| {
                tracing::info!(
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
                    mining_target_id: 0,
                    mining_active: 0,
                }
            });

            // Re-apply possession flag if this is the player ship.
            // This handles cases where Possession arrived before the entity was spawned.
            let is_player = Some(update.network_id) == self.player_network_id;
            if is_player {
                tracing::info!(
                    network_id = update.network_id.0,
                    is_new,
                    "[apply_updates] Setting 0x04 (LocalPlayer) flag on entity"
                );
                entry.flags |= 0x04;
            } else if is_new {
                tracing::info!(
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
        let dt = 0.05; // 20Hz
        let drag_base = 0.05;
        for slot in self.entities.values_mut() {
            // In a simple velocity integration without input, we just move by DX/DY.
            slot.x += slot.dx * dt;
            slot.y += slot.dy * dt;

            // Apply Drag (Local approximation)
            slot.dx *= 1.0 - (drag_base * dt);
            slot.dy *= 1.0 - (drag_base * dt);
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
                "[handle_game_event] POSSESSION received"
            );
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
        }
    }

    fn apply_component_update(&mut self, update: &ComponentUpdate) {
        match update.component_kind {
            // ComponentKind(1) == Transform (Spatial data)
            ComponentKind(1) => match rmp_serde::from_slice::<Transform>(&update.payload) {
                Ok(transform) => {
                    if let Some(entry) = self.entities.get_mut(&update.network_id) {
                        entry.x = transform.x;
                        entry.y = transform.y;
                        entry.z = transform.z;
                        entry.rotation = transform.rotation;
                        if transform.entity_type != 0 {
                            entry.entity_type = transform.entity_type;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode Transform");
                }
            },
            // ComponentKind(5) == ShipClass (Drives rendering type)
            ComponentKind(5) => match rmp_serde::from_slice::<ShipClass>(&update.payload) {
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
            },
            // ComponentKind(3) == ShipStats (HP/Shield)
            ComponentKind(3) => match rmp_serde::from_slice::<ShipStats>(&update.payload) {
                Ok(stats) => {
                    if let Some(entry) = self.entities.get_mut(&update.network_id) {
                        entry.hp = stats.hp;
                        entry.shield = stats.shield;
                    }
                }
                Err(e) => {
                    tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode ShipStats");
                }
            },
            // ComponentKind(1024) == MiningBeam
            aetheris_protocol::types::MINING_BEAM_KIND => {
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
                        tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode MiningBeam");
                    }
                }
            }
            // ComponentKind(1025) == CargoHold
            aetheris_protocol::types::CARGO_HOLD_KIND => {
                use aetheris_protocol::types::CargoHold;
                match rmp_serde::from_slice::<CargoHold>(&update.payload) {
                    Ok(cargo) => {
                        if let Some(entry) = self.entities.get_mut(&update.network_id) {
                            entry.cargo_ore = cargo.ore_count;
                            // High-assurance possession: CargoHold is only replicated to owners.
                            // Flagging this entity as the player for camera tracking and local input handling.
                            entry.flags |= 0x04;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(network_id = update.network_id.0, error = ?e, "Failed to decode CargoHold");
                    }
                }
            }
            // ComponentKind(1026) == Asteroid
            aetheris_protocol::types::ASTEROID_KIND => {
                if let Some(entry) = self.entities.get_mut(&update.network_id) {
                    // Mark as Asteroid type (5) if not already set
                    if entry.entity_type == 0 {
                        entry.entity_type = 5;
                    }
                }
            }
            // ComponentKind(130) == RoomBounds
            aetheris_protocol::types::ROOM_BOUNDS_KIND => {
                use aetheris_protocol::types::RoomBounds;
                if let (Ok(bounds), Some(ptr_val)) = (
                    rmp_serde::from_slice::<RoomBounds>(&update.payload),
                    self.shared_world_ref,
                ) {
                    let mut sw =
                        unsafe { crate::shared_world::SharedWorld::from_ptr(ptr_val as *mut u8) };
                    sw.set_room_bounds(bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y);
                }
            }
            kind => {
                tracing::debug!(
                    network_id = update.network_id.0,
                    kind = kind.0,
                    "Unhandled component kind"
                );
            }
        }
    }
}

impl ClientWorld {
    /// Applies playground input to the local player entity.
    /// Used for Sandbox mode simulation.
    pub fn playground_apply_input(&mut self, move_x: f32, move_y: f32, actions_mask: u32) {
        // Physics constants from VS-01
        const THRUST_ACCEL: f32 = 0.12;
        const DRAG: f32 = 0.92;
        const MAX_SPEED: f32 = 3.0;

        // Locate local player entity (flag 0x04) in the world state
        for slot in self.entities.values_mut() {
            if (slot.flags & 0x04) != 0 {
                // Apply thrust
                slot.dx = (slot.dx + move_x * THRUST_ACCEL) * DRAG;
                slot.dy = (slot.dy + move_y * THRUST_ACCEL) * DRAG;

                // Clamp speed
                let speed_sq = slot.dx * slot.dx + slot.dy * slot.dy;
                if speed_sq > MAX_SPEED * MAX_SPEED {
                    let speed = speed_sq.sqrt();
                    slot.dx = (slot.dx / speed) * MAX_SPEED;
                    slot.dy = (slot.dy / speed) * MAX_SPEED;
                }

                // Integrate position
                slot.x += slot.dx;
                slot.y += slot.dy;

                // Reset mining state if ToggleMining action is triggered in sandbox
                if (actions_mask & 0x02) != 0 {
                    slot.mining_active = 0;
                    slot.mining_target_id = 0;
                }

                break;
            }
        }
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
        let mut world = ClientWorld::new();

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
        // dx = (0 + 1 * 0.12) * 0.92 = 0.1104
        assert!((player.dx - 0.1104).abs() < 0.0001);
    }

    #[test]
    fn test_playground_speed_clamp() {
        let mut world = ClientWorld::new();
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
        assert!(speed <= 3.0 + 0.0001);
    }

    #[test]
    fn test_playground_drag() {
        let mut world = ClientWorld::new();
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
        assert!((v2 - v1 * 0.92).abs() < 0.0001);
    }
}
