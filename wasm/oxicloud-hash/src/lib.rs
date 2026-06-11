//! BLAKE3 for the OxiCloud web frontend.
//!
//! Compiled from the same `blake3` crate the server uses, so a hash
//! computed in the browser equals the server's content address bit for
//! bit — the property the instant-upload path depends on.
//!
//! The API is incremental on purpose: the worker feeds the file in
//! slices (`Blob.slice().arrayBuffer()`), keeping RAM constant no matter
//! how large the file is.

use wasm_bindgen::prelude::*;

/// Incremental BLAKE3 hasher.
///
/// ```js
/// const h = new Blake3Hasher();
/// h.update(chunkBytes);   // repeat per slice
/// const hex = h.finalizeHex();
/// ```
#[wasm_bindgen]
pub struct Blake3Hasher {
    inner: blake3::Hasher,
}

#[wasm_bindgen]
impl Blake3Hasher {
    /// Create a fresh hasher.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Blake3Hasher {
        Blake3Hasher {
            inner: blake3::Hasher::new(),
        }
    }

    /// Feed one slice of the file.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finish and return the lowercase hex digest (64 chars). The hasher
    /// can keep receiving `update` calls afterwards (BLAKE3 finalization
    /// is non-destructive), but the frontend treats it as terminal.
    #[wasm_bindgen(js_name = finalizeHex)]
    pub fn finalize_hex(&self) -> String {
        self.inner.finalize().to_hex().to_string()
    }

    /// Bytes hashed so far — lets the worker report progress without
    /// tracking its own counter.
    pub fn count(&self) -> f64 {
        self.inner.count() as f64
    }
}

impl Default for Blake3Hasher {
    fn default() -> Self {
        Self::new()
    }
}

/// One-shot convenience for small buffers.
#[wasm_bindgen(js_name = blake3Hex)]
pub fn blake3_hex(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector the frontend smoke test uses — also proves the wasm
    /// build hashes identically to the server (same crate, same output).
    #[test]
    fn hello_world_vector() {
        let hasher = {
            let mut h = Blake3Hasher::new();
            h.update(b"Hello, ");
            h.update(b"World!");
            h
        };
        assert_eq!(
            hasher.finalize_hex(),
            "288a86a79f20a3d6dccdca7713beaed178798296bdfa7913fa2a62d9727bf8f8"
        );
        assert_eq!(
            blake3_hex(b"Hello, World!"),
            "288a86a79f20a3d6dccdca7713beaed178798296bdfa7913fa2a62d9727bf8f8"
        );
    }

    #[test]
    fn empty_input_vector() {
        assert_eq!(
            blake3_hex(b""),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }
}
