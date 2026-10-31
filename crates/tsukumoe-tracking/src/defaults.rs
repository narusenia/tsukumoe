// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The default mapping: Live2D standard parameter IDs, plus ARKit-named
//! parameters (Perfect Sync) fed by the blendshape of the same name.
//!
//! Left/right follow the ARKit names (`...Left` feeds `...L...`), and the
//! avatar mirrors the performer like a mirror image: the performer's right is
//! the screen's right. Head signs follow [`crate::euler`]: yaw > 0 turns
//! toward the performer's right (`ParamAngleX` > 0), a pitch > 0 nose-down
//! needs the minus sign for `ParamAngleY` (up is positive in Live2D), roll > 0
//! tilts to the performer's left (assumed `ParamAngleZ` > 0). These signs are
//! unverified against real tracking; flip a weight in the JSON override.

use crate::frame::ARKIT_NAMES;
use crate::rule::{HEAD_PITCH, HEAD_ROLL, HEAD_YAW, Rule, Term};

fn rule(output: &str, offset: f32, terms: &[(&str, f32)]) -> Rule {
    let terms = terms.iter().map(|&(i, w)| Term::new(i, w)).collect();
    Rule::new(output, terms, offset)
}

/// The default rules, Live2D IDs first.
pub fn default_rules() -> Vec<Rule> {
    let mut rules = vec![
        rule("ParamAngleX", 0.0, &[(HEAD_YAW, 1.0)]),
        rule("ParamAngleY", 0.0, &[(HEAD_PITCH, -1.0)]),
        rule("ParamAngleZ", 0.0, &[(HEAD_ROLL, 1.0)]),
        rule("ParamEyeLOpen", 1.0, &[("eyeBlinkLeft", -1.0)]),
        rule("ParamEyeROpen", 1.0, &[("eyeBlinkRight", -1.0)]),
        // Gaze right: the left eye looks in, the right eye looks out.
        rule(
            "ParamEyeBallX",
            0.0,
            &[
                ("eyeLookInLeft", 0.5),
                ("eyeLookOutRight", 0.5),
                ("eyeLookOutLeft", -0.5),
                ("eyeLookInRight", -0.5),
            ],
        ),
        rule(
            "ParamEyeBallY",
            0.0,
            &[
                ("eyeLookUpLeft", 0.5),
                ("eyeLookUpRight", 0.5),
                ("eyeLookDownLeft", -0.5),
                ("eyeLookDownRight", -0.5),
            ],
        ),
        rule(
            "ParamBrowLY",
            0.0,
            &[("browOuterUpLeft", 1.0), ("browDownLeft", -1.0)],
        ),
        rule(
            "ParamBrowRY",
            0.0,
            &[("browOuterUpRight", 1.0), ("browDownRight", -1.0)],
        ),
        rule("ParamMouthOpenY", 0.0, &[("jawOpen", 1.0)]),
        rule(
            "ParamMouthForm",
            0.0,
            &[
                ("mouthSmileLeft", 0.5),
                ("mouthSmileRight", 0.5),
                ("mouthFrownLeft", -0.5),
                ("mouthFrownRight", -0.5),
            ],
        ),
    ];
    rules.extend(ARKIT_NAMES.iter().map(|n| rule(n, 0.0, &[(n, 1.0)])));
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_rule_is_valid_and_outputs_are_unique() {
        let rules = default_rules();
        assert_eq!(rules.len(), 11 + 52);
        for r in &rules {
            assert_eq!(r.validate(), Ok(()), "{}", r.output);
        }
        let mut names: Vec<_> = rules.iter().map(|r| &r.output).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), rules.len());
    }
}
