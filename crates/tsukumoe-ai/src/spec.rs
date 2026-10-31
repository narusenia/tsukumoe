// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Model (weight) descriptions and the JSON manifest.

use serde::Deserialize;

use crate::error::AiError;

/// License facts shown to the user; the full text is not carried.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    /// SPDX identifier, e.g. `MIT`.
    pub spdx: String,
    /// Human-readable name.
    pub name: String,
    /// Where the license text can be read (`https`).
    pub url: String,
}

/// One downloadable weight file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpec {
    pub id: String,
    pub version: String,
    pub file_name: String,
    /// Exact size in bytes.
    pub size: u64,
    /// Lowercase or uppercase hex, 64 digits.
    pub sha256: String,
    pub license: License,
    /// Download URL (`https`).
    pub url: String,
}

/// What the caller shows before a download is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseNotice {
    pub model_id: String,
    pub version: String,
    pub size: u64,
    pub source_url: String,
    pub license_spdx: String,
    pub license_name: String,
    pub license_url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    models: Vec<ModelSpec>,
}

/// Parses and validates a manifest `{"models": [...]}`. Never panics.
pub fn parse_manifest(json: &str) -> Result<Vec<ModelSpec>, AiError> {
    let m: Manifest = serde_json::from_str(json).map_err(|e| AiError::Manifest(e.to_string()))?;
    for spec in &m.models {
        spec.validate()?;
    }
    Ok(m.models)
}

fn bad(msg: String) -> AiError {
    AiError::InvalidSpec(msg)
}

/// A single safe path component: ASCII alphanumerics, `.`, `_`, `-`; not
/// empty, no `..`, not starting with `.`.
fn check_component(what: &str, s: &str) -> Result<(), AiError> {
    let ok = !s.is_empty()
        && !s.starts_with('.')
        && !s.contains("..")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(bad(format!("{what} {s:?} is not a safe name")))
    }
}

fn check_https(what: &str, s: &str) -> Result<(), AiError> {
    let rest = s.get(..8).filter(|p| p.eq_ignore_ascii_case("https://"));
    let ok = rest.is_some() && s.len() > 8 && s.bytes().all(|b| b.is_ascii_graphic());
    if ok {
        Ok(())
    } else {
        Err(bad(format!("{what} must be an https URL")))
    }
}

impl ModelSpec {
    /// Checks every field; called by the manifest parser and the store.
    pub fn validate(&self) -> Result<(), AiError> {
        check_component("id", &self.id)?;
        check_component("version", &self.version)?;
        check_component("file_name", &self.file_name)?;
        if self.size == 0 {
            return Err(bad("size must be positive".into()));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad("sha256 must be 64 hex digits".into()));
        }
        check_https("url", &self.url)?;
        if self.license.spdx.trim().is_empty() || self.license.name.trim().is_empty() {
            return Err(bad("license spdx and name must not be empty".into()));
        }
        check_https("license url", &self.license.url)
    }

    /// Information to show before `ModelStore::ensure` fetches anything.
    pub fn license_notice(&self) -> LicenseNotice {
        LicenseNotice {
            model_id: self.id.clone(),
            version: self.version.clone(),
            size: self.size,
            source_url: self.url.clone(),
            license_spdx: self.license.spdx.clone(),
            license_name: self.license.name.clone(),
            license_url: self.license.url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn model(over: &[(&str, &str)]) -> String {
        let mut f = vec![
            ("id", "\"lipsync-small\"".into()),
            ("version", "\"1.0\"".into()),
            ("file_name", "\"model.onnx\"".into()),
            ("size", "3".into()),
            ("sha256", format!("\"{HASH}\"")),
            ("url", "\"https://example.com/m.onnx\"".into()),
            (
                "license",
                "{\"spdx\":\"MIT\",\"name\":\"MIT License\",\"url\":\"https://example.com/l\"}"
                    .into(),
            ),
        ];
        for (k, v) in over {
            f.retain(|(key, _)| key != k);
            if *v != "<missing>" {
                f.push((k, (*v).to_string()));
            }
        }
        let body: Vec<_> = f.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
        format!("{{\"models\":[{{{}}}]}}", body.join(","))
    }

    fn parse(over: &[(&str, &str)]) -> Result<Vec<ModelSpec>, AiError> {
        parse_manifest(&model(over))
    }

    #[test]
    fn valid_manifest_parses() {
        let specs = parse(&[]).unwrap();
        assert_eq!(specs[0].id, "lipsync-small");
        let n = specs[0].license_notice();
        assert_eq!(n.license_spdx, "MIT");
        assert_eq!(n.size, 3);
    }

    #[test]
    fn missing_field_is_rejected() {
        for k in [
            "id",
            "version",
            "file_name",
            "size",
            "sha256",
            "url",
            "license",
        ] {
            assert!(
                matches!(parse(&[(k, "<missing>")]), Err(AiError::Manifest(_))),
                "{k}"
            );
        }
    }

    #[test]
    fn unsafe_names_are_rejected() {
        for bad in [
            "\"..\"",
            "\"a/b\"",
            "\"a\\b\"",
            "\"\"",
            "\".hidden\"",
            "\"a..b\"",
            "\"é\"",
        ] {
            for k in ["id", "version", "file_name"] {
                assert!(parse(&[(k, bad)]).is_err(), "{k} {bad}");
            }
        }
    }

    #[test]
    fn non_https_urls_are_rejected() {
        for u in [
            "\"http://example.com/m\"",
            "\"file:///etc/passwd\"",
            "\"https://\"",
            "\"\"",
            "\"https://a b\"",
        ] {
            assert!(parse(&[("url", u)]).is_err(), "{u}");
        }
        let lic = "{\"spdx\":\"MIT\",\"name\":\"n\",\"url\":\"http://x\"}";
        assert!(parse(&[("license", lic)]).is_err());
    }

    #[test]
    fn bad_size_and_hash_are_rejected() {
        assert!(parse(&[("size", "0")]).is_err());
        assert!(parse(&[("size", "-1")]).is_err());
        assert!(parse(&[("sha256", "\"abc\"")]).is_err());
        let long = format!("\"{HASH}0\"");
        assert!(parse(&[("sha256", &long)]).is_err());
        let nonhex = format!("\"{}\"", "g".repeat(64));
        assert!(parse(&[("sha256", &nonhex)]).is_err());
    }

    #[test]
    fn garbage_input_does_not_panic() {
        for s in [
            "",
            "{",
            "null",
            "[]",
            "{\"models\":3}",
            "{\"models\":[null]}",
            "\u{0}",
        ] {
            assert!(parse_manifest(s).is_err(), "{s:?}");
        }
    }
}
