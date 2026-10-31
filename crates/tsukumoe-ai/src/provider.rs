// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The provider trait and the data it moves.

use crate::error::AiError;

/// A named, shaped, flat `f32` array (row-major). Only `f32` is supported;
/// other element types are added when a feature needs them.
#[derive(Debug, Clone, PartialEq)]
pub struct Tensor {
    pub name: String,
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl Tensor {
    /// Builds a tensor, rejecting a shape whose element count (or its
    /// overflow) does not equal `data.len()`.
    pub fn new(
        name: impl Into<String>,
        shape: Vec<usize>,
        data: Vec<f32>,
    ) -> Result<Self, AiError> {
        let count = shape
            .iter()
            .try_fold(1usize, |acc, &d| acc.checked_mul(d))
            .ok_or_else(|| AiError::InvalidTensor("shape overflows".into()))?;
        if count != data.len() {
            return Err(AiError::InvalidTensor(format!(
                "shape {shape:?} needs {count} values, got {}",
                data.len()
            )));
        }
        Ok(Self {
            name: name.into(),
            shape,
            data,
        })
    }
}

/// Where inference runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locality {
    /// On this machine; nothing leaves it.
    Local,
    /// On a remote service; input data is sent over the network.
    Cloud,
}

/// What a cloud provider must tell the user before it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disclosure {
    /// Name of the service that receives the data.
    pub destination: String,
    /// Kind of data that is sent (e.g. "microphone audio").
    pub data_kind: String,
}

/// An inference backend. Synchronous only.
pub trait Provider {
    fn locality(&self) -> Locality;

    /// Required for [`Locality::Cloud`]; ignored for `Local`.
    fn disclosure(&self) -> Option<&Disclosure> {
        None
    }

    /// Runs the model on named inputs.
    fn run(&self, inputs: &[Tensor]) -> Result<Vec<Tensor>, AiError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Doubles the single input "x" and renames it "y".
    pub(crate) struct Doubler;

    impl Provider for Doubler {
        fn locality(&self) -> Locality {
            Locality::Local
        }
        fn run(&self, inputs: &[Tensor]) -> Result<Vec<Tensor>, AiError> {
            let [x] = inputs else {
                return Err(AiError::ShapeMismatch("expected one input".into()));
            };
            if x.name != "x" {
                return Err(AiError::ShapeMismatch(format!("unknown input {}", x.name)));
            }
            let data = x.data.iter().map(|v| v * 2.0).collect();
            Ok(vec![Tensor::new("y", x.shape.clone(), data)?])
        }
    }

    #[test]
    fn tensor_rejects_bad_shapes() {
        assert!(Tensor::new("a", vec![2, 3], vec![0.0; 6]).is_ok());
        assert!(Tensor::new("a", vec![2, 3], vec![0.0; 5]).is_err());
        assert!(Tensor::new("a", vec![usize::MAX, 2], vec![]).is_err());
        assert!(Tensor::new("a", vec![0], vec![]).is_ok());
    }

    #[test]
    fn provider_runs_through_the_trait() {
        let p: Box<dyn Provider> = Box::new(Doubler);
        let x = Tensor::new("x", vec![2], vec![1.0, 2.5]).unwrap();
        let out = p.run(&[x]).unwrap();
        assert_eq!(out[0].name, "y");
        assert_eq!(out[0].data, vec![2.0, 5.0]);
        assert_eq!(p.locality(), Locality::Local);
        assert!(p.disclosure().is_none());
    }

    #[test]
    fn provider_reports_input_mismatch() {
        let p = Doubler;
        assert!(p.run(&[]).is_err());
        let z = Tensor::new("z", vec![1], vec![0.0]).unwrap();
        assert!(matches!(p.run(&[z]), Err(AiError::ShapeMismatch(_))));
    }
}
