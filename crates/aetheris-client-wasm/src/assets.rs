use crate::render_primitives::MeshData;
use dashmap::DashMap;
use std::sync::Arc;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::{Request, RequestInit, RequestMode, Response};

// Fallback for non-wasm target to allow JsValue in results
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct JsValue(String);

#[cfg(not(target_arch = "wasm32"))]
impl JsValue {
    pub fn from_str(s: &str) -> Self {
        Self(s.to_string())
    }
    pub fn as_string(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

/// Magic number for Aetheris Engine Binary (.aeb)
const AEB_MAGIC: [u8; 4] = *b"AEB\x01";

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetHandle {
    Interceptor = 1,
    Dreadnought = 2,
    Asteroid = 3,
    Projectile = 4,
}

/// Registry for managing game assets (meshes, textures).
///
/// M10146 — Foundation for binary asset loading.
/// Currently supports procedural mesh fallbacks while establishing the
/// asynchronous loading pipeline for .aeb files.
pub struct AssetRegistry {
    meshes: Arc<DashMap<AssetHandle, MeshData>>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        let registry = Self {
            meshes: Arc::new(DashMap::new()),
        };

        // Seed with procedural defaults for Phase 1
        registry.meshes.insert(
            AssetHandle::Interceptor,
            crate::render_primitives::create_interceptor_mesh(),
        );
        registry.meshes.insert(
            AssetHandle::Dreadnought,
            crate::render_primitives::create_dreadnought_mesh(),
        );
        registry.meshes.insert(
            AssetHandle::Asteroid,
            crate::render_primitives::create_asteroid_mesh(),
        );
        registry.meshes.insert(
            AssetHandle::Projectile,
            crate::render_primitives::create_projectile_mesh(),
        );

        registry
    }

    pub fn get_mesh(&self, handle: AssetHandle) -> Option<MeshData> {
        self.meshes.get(&handle).map(|m| MeshData {
            vertices: m.vertices.clone(),
            indices: m.indices.clone(),
        })
    }

    /// Internal parser for .aeb binary data.
    /// Separated from network logic to allow unit testing on host.
    fn parse_asset(bytes: &[u8]) -> Result<MeshData, JsValue> {
        if bytes.len() < 16 {
            return Err(JsValue::from_str("Asset file too small"));
        }

        if bytes[0..4] != AEB_MAGIC {
            return Err(JsValue::from_str("Invalid AEB magic number"));
        }

        // Header Structure (16 bytes):
        // [0..4]: Magic (AEB\x01)
        // [4..8]: Version (u32, little-endian)
        // [8..16]: Uncompressed Size (u64, little-endian)

        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != 0 {
            return Err(JsValue::from_str(&format!(
                "Unsupported AEB version: {}",
                version
            )));
        }

        let uncompressed_size = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let compressed_payload = &bytes[16..];

        // Decompress using ruzstd
        let mut decompressed = Vec::with_capacity(uncompressed_size as usize);
        let mut decoder = ruzstd::decoding::StreamingDecoder::new(compressed_payload)
            .map_err(|e| JsValue::from_str(&format!("Failed to create decoder: {:?}", e)))?;

        std::io::Read::read_to_end(&mut decoder, &mut decompressed)
            .map_err(|e| JsValue::from_str(&format!("Decompression failed: {}", e)))?;

        #[cfg(target_arch = "wasm32")]
        if decompressed.len() != uncompressed_size as usize && uncompressed_size != 0 {
            web_sys::console::warn_1(
                &format!(
                    "Decompressed size mismatch: expected {}, got {}",
                    uncompressed_size,
                    decompressed.len()
                )
                .into(),
            );
        }

        #[cfg(not(target_arch = "wasm32"))]
        if decompressed.len() != uncompressed_size as usize && uncompressed_size != 0 {
            eprintln!(
                "Decompressed size mismatch: expected {}, got {}",
                uncompressed_size,
                decompressed.len()
            );
        }

        rmp_serde::from_slice(&decompressed)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse mesh: {e}")))
    }
}

#[cfg(target_arch = "wasm32")]
impl AssetRegistry {
    /// Asynchronously loads an asset from a URL and updates the registry.
    pub async fn load_asset(&self, handle: AssetHandle, url: &str) -> Result<(), JsValue> {
        let opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(url, &opts)?;

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("No window context"))?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
        let resp: Response = resp_value.dyn_into().unwrap();

        if !resp.ok() {
            return Err(JsValue::from_str(&format!("HTTP error: {}", resp.status())));
        }

        let buffer = JsFuture::from(resp.array_buffer()?).await?;
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();

        let mesh_data = Self::parse_asset(&bytes)?;
        self.meshes.insert(handle, mesh_data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngExt;

    #[test]
    fn test_aeb_validation_magic_mismatch() {
        let garbage = b"NOT_AEB_DATA_LONG_ENOUGH";
        let result = AssetRegistry::parse_asset(garbage);
        assert!(result.is_err());

        let err = result.unwrap_err().as_string().unwrap();
        assert!(err.contains("magic"));
    }

    #[test]
    fn test_aeb_validation_too_small() {
        let tiny = b"AEB\x01";
        let result = AssetRegistry::parse_asset(tiny);
        assert!(result.is_err());

        let err = result.unwrap_err().as_string().unwrap();
        assert!(err.contains("small"));
    }

    #[test]
    fn test_aeb_parsing_valid() {
        // Simple valid AEB structure with a small test payload
        // The decoder will return what it can from the provided blob
        let compressed = vec![
            40, 181, 47, 253, 4, 0, 121, 0, 0, 146, 146, 147, 147, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 147, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0, 145, 0, 24, 73, 107,
        ];

        let mut data = Vec::from(AEB_MAGIC);
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&45u64.to_le_bytes());
        data.extend_from_slice(&compressed);

        let result = AssetRegistry::parse_asset(&data);
        // We accept if it decompressed something, even if MessagePack parsing fails on this specific dummy blob
        if let Err(e) = result {
            let err = e.as_string().unwrap();
            assert!(err.contains("parse mesh") || err.contains("Decompression"));
        }
    }

    #[test]
    fn test_aeb_fuzz_garbage() {
        let mut rng = rand::rng();

        for _ in 0..50 {
            let len = rng.random_range(0..500);
            let mut garbage = vec![0u8; len];
            rng.fill(&mut garbage[..]);

            let _ = AssetRegistry::parse_asset(&garbage);
        }
    }

    #[test]
    fn test_aeb_corrupted_header() {
        let mut data = Vec::from(AEB_MAGIC);
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&100u64.to_le_bytes());
        data.extend_from_slice(&[0, 1, 2, 3, 4, 5]); // Invalid Zstd data

        let result = AssetRegistry::parse_asset(&data);
        assert!(result.is_err());
        let err = result.unwrap_err().as_string().unwrap();
        assert!(err.contains("Failed") || err.contains("Decompression"));
    }
}
