// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The crate-wide error type.

use std::fmt;

/// Everything that can go wrong in this crate. Untrusted input (manifests,
/// fetched bytes) only ever produces one of these, never a panic.
#[derive(Debug)]
pub enum AiError {
    /// A tensor's data length does not match its shape.
    InvalidTensor(String),
    /// A provider rejected its inputs (count, name or shape).
    ShapeMismatch(String),
    /// A model spec or manifest failed validation.
    InvalidSpec(String),
    /// The manifest is not valid JSON of the expected form.
    Manifest(String),
    /// The user declined a confirmation prompt.
    Declined,
    /// The fetched stream is larger than the spec's size.
    SizeExceeded { expected: u64 },
    /// The fetched stream is smaller than the spec's size.
    SizeMismatch { expected: u64, actual: u64 },
    /// The SHA-256 of the fetched bytes differs from the spec.
    HashMismatch,
    /// The fetcher failed to open or read the source.
    Fetch(String),
    /// Filesystem failure in the store.
    Io(String),
    /// No provider is registered for the feature.
    UnknownFeature(String),
    /// A cloud provider was registered without a disclosure.
    MissingDisclosure,
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTensor(m) => write!(f, "invalid tensor: {m}"),
            Self::ShapeMismatch(m) => write!(f, "input mismatch: {m}"),
            Self::InvalidSpec(m) => write!(f, "invalid model spec: {m}"),
            Self::Manifest(m) => write!(f, "invalid manifest: {m}"),
            Self::Declined => write!(f, "declined by the user"),
            Self::SizeExceeded { expected } => {
                write!(f, "download exceeds the declared size of {expected} bytes")
            }
            Self::SizeMismatch { expected, actual } => {
                write!(f, "download is {actual} bytes, expected {expected}")
            }
            Self::HashMismatch => write!(f, "SHA-256 mismatch"),
            Self::Fetch(m) => write!(f, "fetch failed: {m}"),
            Self::Io(m) => write!(f, "store I/O failed: {m}"),
            Self::UnknownFeature(n) => write!(f, "no provider registered for feature `{n}`"),
            Self::MissingDisclosure => write!(f, "cloud provider has no disclosure"),
        }
    }
}

impl std::error::Error for AiError {}
