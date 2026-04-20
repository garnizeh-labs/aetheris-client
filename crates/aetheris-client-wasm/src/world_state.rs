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
    /// The latest tick received from the server.
    pub latest_tick: u64,
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
            latest_tick: 0,
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
        for (_, update) in updates {
            if update.tick > self.latest_tick {
                self.latest_tick = update.tick;
            }

            // Ensure entity exists for component updates
            let is_new = !self.entities.contains_key(&update.network_id);
            if is_new {
                tracing::debug!(
                    network_id = update.network_id.0,
                    kind = update.component_kind.0,
                    "New entity from server"
                );
            }
            self.entities.entry(update.network_id).or_insert(SabSlot {
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
                padding: [0; 5],
            });

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
                // ComponentKind(6) == ShipStats (HP/Shield)
                ComponentKind(6) => match rmp_serde::from_slice::<ShipStats>(&update.payload) {
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

    fn simulate(&mut self) {}

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
