// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Per-model JSON overrides of the default mapping. The JSON comes from
//! outside, so it is checked on load: size, rule and term counts, unknown
//! names, non-finite numbers. Nothing here panics.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::defaults::default_rules;
use crate::rule::{MappingError, Rule};

/// Largest accepted JSON text, in bytes.
pub const MAX_JSON_BYTES: usize = 1 << 20;
/// Most rules in one override.
pub const MAX_RULES: usize = 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Replace the default rule of the same output, add the rest.
    #[default]
    Merge,
    /// Use only the given rules.
    Replace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub mode: Mode,
}

impl Overrides {
    pub fn from_json(json: &str) -> Result<Self, MappingError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(MappingError::TooLarge);
        }
        let o: Self = serde_json::from_str(json).map_err(|e| MappingError::Json(e.to_string()))?;
        if o.rules.len() > MAX_RULES {
            return Err(MappingError::TooManyRules);
        }
        o.rules.iter().try_for_each(Rule::validate)?;
        Ok(o)
    }

    /// The rules to bind: the defaults with these applied, or only these.
    pub fn resolve(&self) -> Vec<Rule> {
        let mut rules = match self.mode {
            Mode::Merge => default_rules(),
            Mode::Replace => Vec::new(),
        };
        for r in &self.rules {
            rules.retain(|d| d.output != r.output);
            rules.push(r.clone());
        }
        rules
    }
}

/// The JSON Schema of the override file, as JSON text.
pub fn json_schema() -> String {
    serde_json::to_string_pretty(&schemars::schema_for!(Overrides)).expect("schema is JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"{"rules":[{"output":"ParamMouthOpenY","terms":[{"input":"jawOpen","weight":2.0}],"max":1.0}]}"#;

    #[test]
    fn merge_replaces_same_output_and_keeps_the_rest() {
        let o = Overrides::from_json(ONE).unwrap();
        assert_eq!(o.mode, Mode::Merge);
        let rules = o.resolve();
        let defaults = default_rules();
        assert_eq!(rules.len(), defaults.len());
        let mouth: Vec<_> = rules
            .iter()
            .filter(|r| r.output == "ParamMouthOpenY")
            .collect();
        assert_eq!(mouth.len(), 1);
        assert_eq!(mouth[0].terms[0].weight, 2.0);
        assert_eq!(mouth[0].max, Some(1.0));
        assert!(rules.iter().any(|r| r.output == "ParamAngleX"));
    }

    #[test]
    fn merge_adds_new_outputs_and_replace_drops_defaults() {
        let json = ONE.replace("ParamMouthOpenY", "Custom");
        let rules = Overrides::from_json(&json).unwrap().resolve();
        assert_eq!(rules.len(), default_rules().len() + 1);
        let json = json.replace("]}", r#"],"mode":"replace"}"#);
        let rules = Overrides::from_json(&json).unwrap().resolve();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].output, "Custom");
    }

    #[test]
    fn untrusted_json_is_rejected_without_panicking() {
        let rule = |extra: &str| {
            format!(
                r#"{{"rules":[{{"output":"P","terms":[{{"input":"jawOpen","weight":1}}]{extra}}}]}}"#
            )
        };
        let many = format!(
            r#"{{"rules":[{}]}}"#,
            vec![r#"{"output":"P","terms":[]}"#; MAX_RULES + 1].join(",")
        );
        let terms = format!(
            r#"{{"rules":[{{"output":"P","terms":[{}]}}]}}"#,
            vec![r#"{"input":"jawOpen","weight":1}"#; 65].join(",")
        );
        let cases = [
            (many, "too many mapping rules"),
            (terms, "too many terms"),
            (rule(r#","offset":1e999"#), "invalid mapping JSON"),
            (rule(r#","min":2,"max":1"#), "min is greater"),
            (rule("").replace("jawOpen", "noSuchShape"), "unknown input"),
            (rule("").replace("\"P\"", "\"\""), "empty output"),
            (rule(r#","bogus":1"#), "invalid mapping JSON"),
            (r#"{"rules":[],"mode":"x"}"#.into(), "invalid mapping JSON"),
            ("[".repeat(100_000), "invalid mapping JSON"),
            ("not json".into(), "invalid mapping JSON"),
            (" ".repeat(MAX_JSON_BYTES + 1), "too large"),
        ];
        for (json, want) in cases {
            let e = Overrides::from_json(&json).unwrap_err().to_string();
            assert!(e.contains(want), "{e} / {want}");
        }
    }

    #[test]
    fn the_schema_names_the_fields() {
        let s = json_schema();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(v["properties"]["rules"].is_object());
        assert!(v["properties"]["mode"].is_object());
        assert!(s.contains("head_yaw"));
    }
}
