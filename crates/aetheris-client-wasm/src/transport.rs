use aetheris_protocol::{
    MAX_SAFE_PAYLOAD_SIZE,
    events::NetworkEvent,
    traits::{GameTransport, TransportError},
    types::ClientId,
};
use async_trait::async_trait;
use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    ReadableStreamDefaultReader, WebTransport, WebTransportBidirectionalStream,
    WebTransportOptions, WritableStreamDefaultWriter,
};

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A lean networking bridge between the Aetheris Client and the browser's WebTransport API.
#[doc(hidden)]
pub struct WebTransportBridge {
    transport: WebTransport,
    datagram_writer: Mutex<WritableStreamDefaultWriter>,
    event_queue: Arc<Mutex<VecDeque<NetworkEvent>>>,
    closed: Arc<Mutex<bool>>,
    worker_id: usize,
}

// SAFETY: In WASM multi-threaded mode (SharedArrayBuffer), each Web Worker has
// its own isolated JS heap. JS objects such as `WebTransport` are NEVER moved
// across workers — only the WASM linear memory is shared.
//
// WebTransportBridge is only valid for use within the single Game Worker that
// created it. It MUST NOT be transferred or accessed from other threads (e.g.
// the Render Worker). The `Send + Sync` bound from `GameTransport` is
// satisfied structurally via `Arc<Mutex<>>` for all shared state; the
// underlying `WebTransport` handle is pin-bound to the creating worker's
// event loop.
unsafe impl Send for WebTransportBridge {}
unsafe impl Sync for WebTransportBridge {}

impl WebTransportBridge {
    pub fn is_closed(&self) -> bool {
        self.closed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .eq(&true)
    }
    /// Creates a new WebTransport connection to the specified URL.
    ///
    /// # Certificate Pinning
    /// To support local development with self-signed certificates, this method accepts
    /// an optional SHA-256 fingerprint of the server's certificate.
    pub async fn connect(url: &str, cert_hash: Option<&[u8]>) -> Result<Self, JsValue> {
        let options = WebTransportOptions::new();

        // 1. Configure certificate pinning if provided
        if let Some(hash) = cert_hash {
            // Must disable pooling for custom hashes
            Reflect::set(&options, &"allowPooling".into(), &false.into())?;

            let hash_obj = Object::new();
            Reflect::set(&hash_obj, &"algorithm".into(), &"sha-256".into())?;
            let hash_uint8 = Uint8Array::from(hash);
            Reflect::set(&hash_obj, &"value".into(), &hash_uint8)?;

            let hashes_array = Array::new();
            hashes_array.push(&hash_obj);
            Reflect::set(&options, &"serverCertificateHashes".into(), &hashes_array)?;
        }

        // 2. Initialize transport
        let transport = WebTransport::new_with_options(url, &options)?;

        // 3. Wait for connection
        JsFuture::from(transport.ready()).await?;

        // 4. Initialise datagram streams
        let datagrams = transport.datagrams();
        let reader = datagrams
            .readable()
            .get_reader()
            .unchecked_into::<ReadableStreamDefaultReader>();
        let writer = datagrams.writable().get_writer()?;

        let event_queue = Arc::new(Mutex::new(VecDeque::new()));
        let closed = Arc::new(Mutex::new(false));

        // 5. Spawn background reading loop
        let read_queue = Arc::clone(&event_queue);
        let read_closed = Arc::clone(&closed);
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let result = JsFuture::from(reader.read()).await;
                match result {
                    Ok(value) => {
                        let done = Reflect::get(&value, &"done".into())
                            .unwrap_or(JsValue::from(false))
                            .as_bool()
                            .unwrap_or(false);

                        if done {
                            if let Ok(mut c) = read_closed.lock() {
                                *c = true;
                            }
                            break;
                        }

                        let chunk = Reflect::get(&value, &"value".into()).unwrap_or(JsValue::NULL);
                        if let Ok(uint8) = chunk.dyn_into::<Uint8Array>() {
                            if uint8.length() > MAX_SAFE_PAYLOAD_SIZE as u32 {
                                web_sys::console::warn_1(
                                    &format!(
                                        "Dropped oversized packet ({} bytes) exceeding MTU ({})",
                                        uint8.length(),
                                        MAX_SAFE_PAYLOAD_SIZE
                                    )
                                    .into(),
                                );
                                continue;
                            }
                            let data = uint8.to_vec();
                            let encoder = aetheris_encoder_serde::SerdeEncoder::new();

                            if let Ok(mut q) = read_queue.lock() {
                                // Attempt to decode as a NetworkEvent (Ping/Pong/etc)
                                if let Ok(event) = encoder.decode_event(&data) {
                                    q.push_back(event);
                                } else {
                                    // Fallback to raw message if it's not a protocol event
                                    q.push_back(NetworkEvent::UnreliableMessage {
                                        client_id: ClientId(0),
                                        data,
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        web_sys::console::error_2(&"WebTransport reader.read() failed:".into(), &e);
                        if let Ok(mut c) = read_closed.lock() {
                            *c = true;
                        }
                        break;
                    }
                }
            }
        });

        // 6. Spawn background reliable stream reading loop
        let stream_read_queue = Arc::clone(&event_queue);
        let stream_closed = Arc::clone(&closed);
        let incoming_streams = transport.incoming_bidirectional_streams();
        let stream_reader = incoming_streams
            .get_reader()
            .unchecked_into::<ReadableStreamDefaultReader>();

        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let result = JsFuture::from(stream_reader.read()).await;
                match result {
                    Ok(value) => {
                        let done = Reflect::get(&value, &"done".into())
                            .unwrap_or(JsValue::from(false))
                            .as_bool()
                            .unwrap_or(false);

                        if done {
                            break;
                        }

                        let bi_stream = Reflect::get(&value, &"value".into())
                            .unwrap_or(JsValue::NULL)
                            .unchecked_into::<WebTransportBidirectionalStream>();

                        let readable = bi_stream.readable();
                        let reader = readable
                            .get_reader()
                            .unchecked_into::<ReadableStreamDefaultReader>();

                        let queue = Arc::clone(&stream_read_queue);
                        wasm_bindgen_futures::spawn_local(async move {
                            let mut buffer = Vec::new();
                            loop {
                                let read_res = JsFuture::from(reader.read()).await;
                                match read_res {
                                    Ok(read_val) => {
                                        let read_done = Reflect::get(&read_val, &"done".into())
                                            .unwrap_or(JsValue::from(false))
                                            .as_bool()
                                            .unwrap_or(false);

                                        if read_done {
                                            break;
                                        }

                                        let chunk = Reflect::get(&read_val, &"value".into())
                                            .unwrap_or(JsValue::NULL);
                                        if let Ok(uint8) = chunk.dyn_into::<Uint8Array>() {
                                            buffer.extend_from_slice(&uint8.to_vec());
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }

                            if !buffer.is_empty() {
                                if let Ok(mut q) = queue.lock() {
                                    q.push_back(NetworkEvent::ReliableMessage {
                                        client_id: ClientId(0),
                                        data: buffer,
                                    });
                                }
                            }
                        });
                    }
                    Err(e) => {
                        web_sys::console::error_2(
                            &"WebTransport incoming streams reader failed:".into(),
                            &e,
                        );
                        if let Ok(mut c) = stream_closed.lock() {
                            *c = true;
                        }
                        break;
                    }
                }
            }
        });

        Ok(Self {
            transport,
            datagram_writer: Mutex::new(writer),
            event_queue,
            closed,
            worker_id: crate::get_worker_id(),
        })
    }

    fn check_worker(&self) {
        assert_eq!(
            self.worker_id,
            crate::get_worker_id(),
            "WebTransportBridge accessed from wrong worker! It is pin-bound to its creating thread."
        );
    }
}

#[async_trait(?Send)]
impl GameTransport for WebTransportBridge {
    async fn send_unreliable(
        &self,
        _client_id: ClientId,
        data: &[u8],
    ) -> Result<(), TransportError> {
        self.check_worker();
        let promise = {
            let writer = self.datagram_writer.lock().map_err(|_| {
                TransportError::Io(std::io::Error::other("Datagram writer mutex poisoned"))
            })?;

            let uint8 = Uint8Array::from(data);
            writer.write_with_chunk(&uint8)
        };

        JsFuture::from(promise).await.map_err(|e| {
            TransportError::Io(std::io::Error::other(format!(
                "WebTransport datagram write failed: {e:?}"
            )))
        })?;

        Ok(())
    }

    async fn send_reliable(&self, _client_id: ClientId, data: &[u8]) -> Result<(), TransportError> {
        self.check_worker();

        // Use a bidirectional stream so the server's accept_bi() loop receives
        // the data. The readable half is unused but required by the server API.
        let bi_stream: WebTransportBidirectionalStream =
            JsFuture::from(self.transport.create_bidirectional_stream())
                .await
                .map_err(|e| {
                    TransportError::Io(std::io::Error::other(format!(
                        "Failed to create bidirectional stream: {e:?}"
                    )))
                })?
                .unchecked_into();

        // Write data to the writable half of the bidirectional stream.
        let writable = bi_stream.writable();
        let writer: WritableStreamDefaultWriter = writable.get_writer().map_err(|e| {
            TransportError::Io(std::io::Error::other(format!(
                "Failed to get stream writer: {e:?}"
            )))
        })?;

        let uint8 = Uint8Array::from(data);
        JsFuture::from(writer.write_with_chunk(&uint8))
            .await
            .map_err(|e| {
                TransportError::Io(std::io::Error::other(format!(
                    "Failed to write to reliable stream: {e:?}"
                )))
            })?;

        JsFuture::from(writer.close()).await.map_err(|e| {
            TransportError::Io(std::io::Error::other(format!(
                "Failed to close reliable stream: {e:?}"
            )))
        })?;

        Ok(())
    }

    async fn broadcast_unreliable(&self, data: &[u8]) -> Result<(), TransportError> {
        self.send_unreliable(ClientId(0), data).await
    }

    async fn poll_events(&mut self) -> Result<Vec<NetworkEvent>, TransportError> {
        self.check_worker();
        let mut q = self.event_queue.lock().map_err(|_| {
            TransportError::Io(std::io::Error::other(
                "WebTransportBridge event_queue mutex is poisoned in poll_events",
            ))
        })?;

        let mut events: Vec<NetworkEvent> = q.drain(..).collect();

        if self.is_closed() {
            events.push(NetworkEvent::Disconnected(ClientId(0)));
        }

        Ok(events)
    }

    async fn connected_client_count(&self) -> usize {
        1 // On the client, we are only ever connected to 1 server.
    }
}
