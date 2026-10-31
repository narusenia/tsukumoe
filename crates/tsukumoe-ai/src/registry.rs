// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Per-feature provider selection with cloud confirmation.

use std::collections::HashMap;

use crate::error::AiError;
use crate::provider::{Disclosure, Locality, Provider};

/// Maps a feature name (e.g. `lipsync`) to the provider that serves it.
///
/// A new registry is empty. The host registers `Local` providers by default
/// and registers a `Cloud` one only when the user has set that up explicitly.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers (or replaces) the provider for `feature`, returning the one
    /// it replaced. A cloud provider without a [`Disclosure`] is refused.
    pub fn register(
        &mut self,
        feature: impl Into<String>,
        provider: Box<dyn Provider>,
    ) -> Result<Option<Box<dyn Provider>>, AiError> {
        if provider.locality() == Locality::Cloud && provider.disclosure().is_none() {
            return Err(AiError::MissingDisclosure);
        }
        Ok(self.providers.insert(feature.into(), provider))
    }

    /// Returns the provider for `feature`. For a cloud provider, `confirm`
    /// sees its disclosure and must return `true`; local providers never
    /// trigger it.
    pub fn acquire(
        &self,
        feature: &str,
        confirm: impl FnOnce(&Disclosure) -> bool,
    ) -> Result<&dyn Provider, AiError> {
        let p = self
            .providers
            .get(feature)
            .ok_or_else(|| AiError::UnknownFeature(feature.to_string()))?;
        if p.locality() == Locality::Cloud {
            let d = p.disclosure().ok_or(AiError::MissingDisclosure)?;
            if !confirm(d) {
                return Err(AiError::Declined);
            }
        }
        Ok(p.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Tensor;

    struct Fixed {
        locality: Locality,
        disclosure: Option<Disclosure>,
        tag: f32,
    }
    impl Provider for Fixed {
        fn locality(&self) -> Locality {
            self.locality
        }
        fn disclosure(&self) -> Option<&Disclosure> {
            self.disclosure.as_ref()
        }
        fn run(&self, _: &[Tensor]) -> Result<Vec<Tensor>, AiError> {
            Ok(vec![Tensor::new("t", vec![1], vec![self.tag])?])
        }
    }

    fn local(tag: f32) -> Box<dyn Provider> {
        Box::new(Fixed {
            locality: Locality::Local,
            disclosure: None,
            tag,
        })
    }
    fn cloud(with_disclosure: bool) -> Box<dyn Provider> {
        Box::new(Fixed {
            locality: Locality::Cloud,
            disclosure: with_disclosure.then(|| Disclosure {
                destination: "Example Cloud".into(),
                data_kind: "audio".into(),
            }),
            tag: 0.0,
        })
    }
    fn tag(p: &dyn Provider) -> f32 {
        p.run(&[]).unwrap()[0].data[0]
    }

    #[test]
    fn unregistered_feature_is_an_error() {
        let r = ProviderRegistry::new();
        assert!(matches!(
            r.acquire("lipsync", |_| true),
            Err(AiError::UnknownFeature(_))
        ));
    }

    #[test]
    fn registering_again_replaces() {
        let mut r = ProviderRegistry::new();
        assert!(r.register("lipsync", local(1.0)).unwrap().is_none());
        assert!(r.register("lipsync", local(2.0)).unwrap().is_some());
        assert_eq!(tag(r.acquire("lipsync", |_| false).unwrap()), 2.0);
    }

    #[test]
    fn local_needs_no_confirmation() {
        let mut r = ProviderRegistry::new();
        r.register("lipsync", local(1.0)).unwrap();
        assert!(r.acquire("lipsync", |_| panic!("must not ask")).is_ok());
    }

    #[test]
    fn cloud_requires_confirmation() {
        let mut r = ProviderRegistry::new();
        r.register("lipsync", cloud(true)).unwrap();
        let mut seen = None;
        let declined = r.acquire("lipsync", |d| {
            seen = Some(d.clone());
            false
        });
        assert!(matches!(declined, Err(AiError::Declined)));
        assert_eq!(seen.unwrap().destination, "Example Cloud");
        assert!(r.acquire("lipsync", |_| true).is_ok());
    }

    #[test]
    fn cloud_without_disclosure_is_refused() {
        let mut r = ProviderRegistry::new();
        assert!(matches!(
            r.register("lipsync", cloud(false)),
            Err(AiError::MissingDisclosure)
        ));
    }
}
