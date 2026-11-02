// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Which deletions ask first. Saved per user in `confirm.json`; a kind that is
//! not listed asks, and a missing or broken file asks for everything.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A kind of deletion that has its own "don't ask again".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Parts, meshes and deformers, from the tree panels.
    Tree,
    Parameter,
    /// A key of a parameter binding.
    Key,
    /// Unbinding a parameter from the selected elements.
    Unbind,
    /// Vertices and edges of the mesh being edited.
    Vertices,
}

pub const KINDS: [Kind; 5] = [
    Kind::Tree,
    Kind::Parameter,
    Kind::Key,
    Kind::Unbind,
    Kind::Vertices,
];

impl Kind {
    /// The translation key of the kind's name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Tree => "confirm-kind-tree",
            Self::Parameter => "confirm-kind-parameter",
            Self::Key => "confirm-kind-key",
            Self::Unbind => "confirm-kind-unbind",
            Self::Vertices => "confirm-kind-vertices",
        }
    }
}

/// The kinds that were switched off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Confirm {
    #[serde(default)]
    off: BTreeSet<Kind>,
}

impl Confirm {
    pub fn asks(&self, kind: Kind) -> bool {
        !self.off.contains(&kind)
    }

    pub fn set(&mut self, kind: Kind, asks: bool) {
        if asks {
            self.off.remove(&kind);
        } else {
            self.off.insert(kind);
        }
    }

    pub fn reset(&mut self) {
        self.off.clear();
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("confirm settings serialize")
    }

    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_asks_until_switched_off_and_a_broken_file_asks_again() {
        let mut c = Confirm::default();
        assert!(KINDS.iter().all(|&k| c.asks(k)));
        c.set(Kind::Vertices, false);
        assert!(!c.asks(Kind::Vertices) && c.asks(Kind::Tree));
        assert_eq!(Confirm::from_json(&c.to_json()), c);
        assert_eq!(Confirm::from_json("{"), Confirm::default());
        assert_eq!(
            Confirm::from_json(r#"{"off":["nope"]}"#),
            Confirm::default()
        );
        c.reset();
        assert!(c.asks(Kind::Vertices));
    }
}
