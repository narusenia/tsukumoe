// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::Error;

pub const FORMAT_NAME: &str = "tsukumoe-project";
/// The newest major version this reader understands.
pub const MAJOR: u32 = 1;
pub const MINOR: u32 = 0;
/// Features this reader implements. None yet.
pub const KNOWN_FEATURES: &[&str] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

/// An optional capability of the format. A reader that does not know a
/// `required` feature must reject the file; one it does not know that is not
/// `required` may be ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Feature {
    pub name: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    /// Always `tsukumoe-project`.
    pub format: String,
    pub version: Version,
    /// The program that wrote the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    #[serde(default)]
    pub features: Vec<Feature>,
}

impl Manifest {
    pub fn new(generator: Option<String>) -> Self {
        Self {
            format: FORMAT_NAME.to_owned(),
            version: Version {
                major: MAJOR,
                minor: MINOR,
            },
            generator,
            features: Vec::new(),
        }
    }

    /// Whether this reader can read the file. Unknown optional features pass.
    pub fn check(&self) -> Result<(), Error> {
        if self.format != FORMAT_NAME {
            return Err(Error::NotAProject);
        }
        if self.version.major == 0 || self.version.major > MAJOR {
            return Err(Error::UnsupportedVersion {
                found: self.version.major,
                supported: MAJOR,
            });
        }
        match self
            .features
            .iter()
            .find(|f| f.required && !KNOWN_FEATURES.contains(&f.name.as_str()))
        {
            Some(f) => Err(Error::RequiredFeature(f.name.clone())),
            None => Ok(()),
        }
    }
}
