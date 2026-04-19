use crate::wasm_impl::{AetherisClient, ConnectionState};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn test_client_initialization() {
    let client = AetherisClient::new(None).expect("Failed to create client");
    assert_eq!(client.connection_state(), ConnectionState::Disconnected);
}

#[wasm_bindgen_test]
async fn test_shared_world_transfer_simulation() {
    let client = AetherisClient::new(None).expect("Failed to create client");

    // In a real environment, this would be transferred to a worker.
    // For the smoke test, we verify we can access the underlying resource.
    let world_ptr = client.shared_world_ptr();
    assert!(world_ptr > 0, "Shared world pointer should not be zero");
}

#[wasm_bindgen_test]
async fn test_auth_client_creation() {
    // Verifies that the gRPC-web client can be instantiated in a WASM environment
    let _client = AetherisClient::new(None).expect("Failed to create client");

    // Use an RFC 2606 reserved domain — guaranteed to never resolve.
    let login_result = AetherisClient::request_otp(
        "http://example.invalid".to_string(),
        "test@example.com".to_string(),
    )
    .await;

    // We expect a failure here because there is no server, but we want to ensure
    // it doesn't panic and returns a proper error from the gRPC stack.
    assert!(login_result.is_err());
}
