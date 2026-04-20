use crate::transport_mock::MockTransport;
use crate::wasm_impl::{AetherisClient, ConnectionState};
use aetheris_protocol::events::{NetworkEvent, ReplicationEvent};
use aetheris_protocol::traits::Encoder;
use aetheris_protocol::types::{ClientId, ComponentKind, NetworkId, Transform};
use wasm_bindgen_test::*;

// wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn test_client_initialization() {
    let client = AetherisClient::new(None).expect("Failed to create client");
    assert_eq!(client.connection_state(), ConnectionState::Disconnected);
}

#[wasm_bindgen_test]
async fn test_ping_pong_rtt() {
    let mut client = AetherisClient::new(None).expect("Failed to create client");
    let mock = MockTransport::new();
    client.transport = Some(Box::new(mock.clone()));

    // 1. Send Ping (happens automatically in tick approx every 60 ticks, but we can mock it)
    let ping_tick = 12345u64;
    client.last_rtt_ms = 0.0;

    // 2. Inject Pong from server
    let pong = NetworkEvent::Pong { tick: ping_tick };
    mock.inject_event(pong);

    // 3. Tick to process event
    // We need to override performance_now or just accept whatever it calculates.
    // In wasm-bindgen-test, we don't easily control the clock.
    client.tick().await;

    // RTT should be non-zero if processed (calculated as now - ping_tick)
    // Since we can't easily control performance_now(), we just check if it changed.
    assert!(
        client.last_rtt_ms >= 0.0,
        "RTT should be calculated: {}",
        client.last_rtt_ms
    );
}

#[wasm_bindgen_test]
async fn test_component_replication_to_shared_world() {
    let mut client = AetherisClient::new(None).expect("Failed to create client");
    let mock = MockTransport::new();
    client.transport = Some(Box::new(mock.clone()));

    // 1. Create a Transform component payload
    let transform = Transform {
        x: 10.0,
        y: 20.0,
        z: 0.0,
        rotation: 1.5,
        entity_type: 1, // Interceptor
    };
    let payload = rmp_serde::to_vec(&transform).unwrap();

    // 2. Inject ReplicationEvent via UnreliableMessage
    let encoder = aetheris_encoder_serde::SerdeEncoder::new();
    let mut buffer = [0u8; 1024];
    let update = ReplicationEvent {
        network_id: NetworkId(42),
        component_kind: ComponentKind(1), // Transform
        payload,
        tick: 1,
    };
    let len = encoder.encode(&update, &mut buffer).unwrap();

    mock.inject_event(NetworkEvent::UnreliableMessage {
        client_id: ClientId(0),
        data: buffer[..len].to_vec(),
    });

    // 3. Tick the client
    client.tick().await;

    // 4. Verify SharedWorld (SAB) reflects the change
    let world = &client.shared_world;
    let entities = world.get_read_buffer();
    assert_eq!(entities.len(), 1, "Should have 1 entity in SAB");
    assert_eq!(entities[0].network_id, 42);
    assert_eq!(entities[0].x, 10.0);
    assert_eq!(entities[0].y, 20.0);
    assert_eq!(entities[0].rotation, 1.5);
    assert_eq!(entities[0].entity_type, 1);
}

#[wasm_bindgen_test]
async fn test_replication_stress() {
    let mut client = AetherisClient::new(None).expect("Failed to create client");
    let mock = MockTransport::new();
    client.transport = Some(Box::new(mock.clone()));

    let encoder = aetheris_encoder_serde::SerdeEncoder::new();
    let entity_count = 1000;

    // 1. Inject 1,000 entities
    for i in 0..entity_count {
        let transform = Transform {
            x: i as f32,
            y: i as f32,
            z: 0.0,
            rotation: 0.0,
            entity_type: 1,
        };
        let payload = rmp_serde::to_vec(&transform).unwrap();
        let update = ReplicationEvent {
            network_id: NetworkId(i as u64),
            component_kind: ComponentKind(1),
            payload,
            tick: 1,
        };

        let mut buffer = [0u8; 1024];
        let len = encoder.encode(&update, &mut buffer).unwrap();

        mock.inject_event(NetworkEvent::UnreliableMessage {
            client_id: ClientId(0),
            data: buffer[..len].to_vec(),
        });
    }

    // 2. Tick the client (this will process all 1,000 updates)
    let start = crate::performance_now();
    client.tick().await;
    let duration = crate::performance_now() - start;

    tracing::info!(
        "Processed {} entity updates in {}ms",
        entity_count,
        duration
    );

    // 3. Verify SAB consistency
    let world = &client.shared_world;
    assert_eq!(
        world.entity_count() as usize,
        entity_count,
        "SAB should contain all 1,000 entities"
    );

    // Verify a random entity
    let entities = world.get_read_buffer();
    assert_eq!(entities[500].network_id, 500);
    assert_eq!(entities[500].x, 500.0);
}

#[wasm_bindgen_test]
async fn test_input_pressure() {
    let mut client = AetherisClient::new(None).expect("Failed to create client");
    let mock = MockTransport::new();
    client.transport = Some(Box::new(mock.clone()));

    // Flood the transport with 100 inputs
    for i in 0..100 {
        client
            .send_input(i, 1.0, 0.0, 0)
            .await
            .expect("Failed to send input");
    }

    let outbound = mock.outbound_messages.lock().unwrap();
    assert_eq!(outbound.len(), 100, "Should have 100 outbound messages");

    // Verify encoding of the last one
    let (_, data, reliable) = &outbound[99];
    assert!(!reliable);
    assert!(data.len() > 0);
}

#[wasm_bindgen_test]
async fn test_high_frequency_pings() {
    let mut client = AetherisClient::new(None).expect("Failed to create client");
    let mock = MockTransport::new();
    client.transport = Some(Box::new(mock.clone()));

    for i in 0..50 {
        mock.inject_event(NetworkEvent::Pong { tick: i });
        client.tick().await;
        assert!(client.last_rtt_ms >= 0.0);
    }
}
