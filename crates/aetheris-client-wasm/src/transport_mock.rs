use aetheris_protocol::events::NetworkEvent;
use aetheris_protocol::traits::{GameTransport, TransportError};
use aetheris_protocol::types::ClientId;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A mock transport for unit testing reconnection and protocol logic.
#[derive(Clone, Default)]
pub struct MockTransport {
    pub inbound_events: Arc<Mutex<VecDeque<NetworkEvent>>>,
    pub outbound_messages: Arc<Mutex<Vec<(ClientId, Vec<u8>, bool)>>>, // (client, data, reliable)
    pub is_closed: Arc<Mutex<bool>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Injects an event to be returned by next poll_events()
    pub fn inject_event(&self, event: NetworkEvent) {
        self.inbound_events.lock().unwrap().push_back(event);
    }

    pub fn set_closed(&self, closed: bool) {
        *self.is_closed.lock().unwrap() = closed;
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl GameTransport for MockTransport {
    async fn send_unreliable(
        &self,
        client_id: ClientId,
        data: &[u8],
    ) -> Result<(), TransportError> {
        self.outbound_messages
            .lock()
            .unwrap()
            .push((client_id, data.to_vec(), false));
        Ok(())
    }

    async fn send_reliable(&self, client_id: ClientId, data: &[u8]) -> Result<(), TransportError> {
        self.outbound_messages
            .lock()
            .unwrap()
            .push((client_id, data.to_vec(), true));
        Ok(())
    }

    async fn broadcast_unreliable(&self, data: &[u8]) -> Result<(), TransportError> {
        self.outbound_messages
            .lock()
            .unwrap()
            .push((ClientId(0), data.to_vec(), false));
        Ok(())
    }

    async fn poll_events(&mut self) -> Result<Vec<NetworkEvent>, TransportError> {
        let mut events: Vec<NetworkEvent> = self.inbound_events.lock().unwrap().drain(..).collect();
        if *self.is_closed.lock().unwrap() {
            events.push(NetworkEvent::Disconnected(ClientId(0)));
        }
        Ok(events)
    }

    async fn connected_client_count(&self) -> usize {
        if *self.is_closed.lock().unwrap() {
            0
        } else {
            1
        }
    }
}
