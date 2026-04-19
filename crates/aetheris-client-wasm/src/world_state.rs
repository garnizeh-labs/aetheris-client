//! Client-side world state and interpolation buffer.
//!
//! This module implements the `WorldState` trait for the WASM client,
//! providing the foundation for the two-tick interpolation buffer.

use crate::shared_world::SabSlot;
use aetheris_protocol::error::WorldError;
use aetheris_protocol::events::{ComponentUpdate, ReplicationEvent};
use aetheris_protocol::traits::WorldState;
use aetheris_protocol::types::{ClientId, ComponentKind, LocalId, NetworkId, Transform};
use std::collections::HashMap;

/// A simplified client-side world that tracks entity states using `SabSlot`.
#[derive(Debug)]
pub struct ClientWorld {
    /// Map of `NetworkId` to the last known authoritative state.
    pub entities: HashMap<NetworkId, SabSlot>,
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
            entities: HashMap::new(),
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

            // ComponentKind(1) == Transform
            if update.component_kind == ComponentKind(1)
                && let Ok(transform) = rmp_serde::from_slice::<Transform>(&update.payload)
            {
                let entry = self.entities.entry(update.network_id).or_insert(SabSlot {
                    network_id: update.network_id.0,
                    x: transform.x,
                    y: transform.y,
                    z: transform.z,
                    rotation: transform.rotation,
                    dx: 0.0,
                    dy: 0.0,
                    dz: 0.0,
                    hp: 100,
                    shield: 0,
                    entity_type: transform.entity_type,
                    flags: 1, // ALIVE
                    padding: [0; 5],
                });

                entry.x = transform.x;
                entry.y = transform.y;
                entry.z = transform.z;
                entry.rotation = transform.rotation;
                entry.entity_type = transform.entity_type;
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
