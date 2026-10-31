// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A mapping rule: one output parameter as a clamped linear combination of
//! tracking inputs. There are no expressions here; anything fancier belongs
//! to the model's parameter expressions.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::frame::arkit_index;

/// Most terms in one rule.
pub const MAX_TERMS: usize = 64;

/// Head angle inputs, in degrees (see [`crate::euler`] for the convention).
pub const HEAD_YAW: &str = "head_yaw";
pub const HEAD_PITCH: &str = "head_pitch";
pub const HEAD_ROLL: &str = "head_roll";

/// A resolved input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Blend(usize),
    Yaw,
    Pitch,
    Roll,
}

/// Resolves an input name: `head_yaw` / `head_pitch` / `head_roll`, or an ARKit
/// blendshape name (ASCII case-insensitive).
pub(crate) fn resolve_input(name: &str) -> Option<Source> {
    match name {
        HEAD_YAW => Some(Source::Yaw),
        HEAD_PITCH => Some(Source::Pitch),
        HEAD_ROLL => Some(Source::Roll),
        _ => arkit_index(name).map(Source::Blend),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Term {
    /// An ARKit blendshape name or `head_yaw` / `head_pitch` / `head_roll`.
    pub input: String,
    pub weight: f32,
}

impl Term {
    pub fn new(input: &str, weight: f32) -> Self {
        Self {
            input: input.into(),
            weight,
        }
    }
}

/// `clamp(offset + sum(weight * input), min, max)` written to `output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// The model parameter's name.
    pub output: String,
    pub terms: Vec<Term>,
    #[serde(default)]
    pub offset: f32,
    #[serde(default)]
    pub min: Option<f32>,
    #[serde(default)]
    pub max: Option<f32>,
}

impl Rule {
    pub fn new(output: &str, terms: Vec<Term>, offset: f32) -> Self {
        Self {
            output: output.into(),
            terms,
            offset,
            min: None,
            max: None,
        }
    }

    /// Rejects what a rule must never contain, so that binding and applying
    /// cannot fail or panic.
    pub fn validate(&self) -> Result<(), MappingError> {
        let out = || self.output.clone();
        if self.output.is_empty() {
            return Err(MappingError::EmptyOutput);
        }
        if self.terms.len() > MAX_TERMS {
            return Err(MappingError::TooManyTerms(out()));
        }
        for t in &self.terms {
            if resolve_input(&t.input).is_none() {
                return Err(MappingError::UnknownInput(t.input.clone()));
            }
            if !t.weight.is_finite() {
                return Err(MappingError::NonFinite(out()));
            }
        }
        let bounds = [self.min, self.max];
        if !self.offset.is_finite() || bounds.iter().flatten().any(|v| !v.is_finite()) {
            return Err(MappingError::NonFinite(out()));
        }
        if matches!(bounds, [Some(a), Some(b)] if a > b) {
            return Err(MappingError::BadRange(out()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MappingError {
    Json(String),
    TooLarge,
    TooManyRules,
    TooManyTerms(String),
    EmptyOutput,
    UnknownInput(String),
    NonFinite(String),
    BadRange(String),
}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid mapping JSON: {e}"),
            Self::TooLarge => f.write_str("mapping JSON is too large"),
            Self::TooManyRules => f.write_str("too many mapping rules"),
            Self::TooManyTerms(o) => write!(f, "too many terms in the rule for `{o}`"),
            Self::EmptyOutput => f.write_str("a rule has an empty output name"),
            Self::UnknownInput(i) => write!(f, "unknown input `{i}`"),
            Self::NonFinite(o) => write!(f, "non-finite number in the rule for `{o}`"),
            Self::BadRange(o) => write!(f, "min is greater than max in the rule for `{o}`"),
        }
    }
}

impl std::error::Error for MappingError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Rule {
        Rule::new("P", vec![Term::new("jawOpen", 1.0)], 0.0)
    }

    #[test]
    fn a_good_rule_passes_and_inputs_resolve() {
        assert_eq!(rule().validate(), Ok(()));
        assert_eq!(resolve_input("JawOpen"), Some(Source::Blend(24)));
        assert_eq!(resolve_input("head_roll"), Some(Source::Roll));
    }

    #[test]
    fn bad_rules_are_rejected() {
        let bad = |f: fn(&mut Rule)| {
            let mut r = rule();
            f(&mut r);
            r.validate().unwrap_err()
        };
        assert_eq!(bad(|r| r.output.clear()), MappingError::EmptyOutput);
        assert!(matches!(
            bad(|r| r.terms[0].input = "nope".into()),
            MappingError::UnknownInput(_)
        ));
        assert!(matches!(
            bad(|r| r.terms[0].weight = f32::NAN),
            MappingError::NonFinite(_)
        ));
        assert!(matches!(
            bad(|r| r.offset = f32::INFINITY),
            MappingError::NonFinite(_)
        ));
        assert!(matches!(
            bad(|r| r.max = Some(f32::NAN)),
            MappingError::NonFinite(_)
        ));
        assert!(matches!(
            bad(|r| {
                r.min = Some(1.0);
                r.max = Some(0.0);
            }),
            MappingError::BadRange(_)
        ));
        assert!(matches!(
            bad(|r| r.terms = vec![Term::new("jawOpen", 1.0); MAX_TERMS + 1]),
            MappingError::TooManyTerms(_)
        ));
    }
}
