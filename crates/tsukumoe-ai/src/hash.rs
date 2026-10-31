// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! SHA-256 over `sha2`; the only cryptography in this crate.

use sha2::{Digest, Sha256};

/// Streaming SHA-256 producing lowercase hex.
#[derive(Default)]
pub struct Sha256Hex(Sha256);

impl Sha256Hex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    pub fn finish(self) -> String {
        self.0
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// One-shot SHA-256 as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256Hex::new();
    h.update(data);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn split_input_matches_whole() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let mut h = Sha256Hex::new();
        for chunk in data.chunks(7) {
            h.update(chunk);
        }
        assert_eq!(h.finish(), sha256_hex(&data));
    }
}
