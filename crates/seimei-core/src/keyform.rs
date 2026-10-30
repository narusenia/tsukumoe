//! Keyform bindings and grids (ADR-0018).
//!
//! A [`Binding`] names the parameters an element reacts to and the key
//! positions on each. The keyforms sit on the grid of all key combinations,
//! stored densely in row-major order: the first axis varies slowest, the last
//! fastest. Elements that react to the same parameters at the same keys share
//! one binding.

use crate::arena::Id;
use crate::elements::ParameterId;

pub type BindingId = Id<Binding>;

#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    pub parameter: ParameterId,
    /// Strictly ascending parameter values.
    pub keys: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    NoAxes,
    EmptyAxis { axis: usize },
    NonFinite { axis: usize },
    NotAscending { axis: usize },
    DuplicateParameter { axis: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    axes: Vec<Axis>,
}

impl Binding {
    pub fn new(axes: Vec<Axis>) -> Result<Self, BindingError> {
        if axes.is_empty() {
            return Err(BindingError::NoAxes);
        }
        for (i, axis) in axes.iter().enumerate() {
            if axis.keys.is_empty() {
                return Err(BindingError::EmptyAxis { axis: i });
            }
            if axis.keys.iter().any(|k| !k.is_finite()) {
                return Err(BindingError::NonFinite { axis: i });
            }
            if axis.keys.windows(2).any(|w| w[0] >= w[1]) {
                return Err(BindingError::NotAscending { axis: i });
            }
            if axes[..i].iter().any(|a| a.parameter == axis.parameter) {
                return Err(BindingError::DuplicateParameter { axis: i });
            }
        }
        Ok(Self { axes })
    }

    pub fn axes(&self) -> &[Axis] {
        &self.axes
    }

    /// Number of keyforms a grid on this binding holds.
    pub fn keyform_count(&self) -> usize {
        self.axes.iter().map(|a| a.keys.len()).product()
    }

    /// Position in the dense array of the grid point with the given key
    /// index on each axis.
    pub fn index(&self, coords: &[usize]) -> Option<usize> {
        if coords.len() != self.axes.len() {
            return None;
        }
        let mut index = 0;
        for (axis, &coord) in self.axes.iter().zip(coords) {
            if coord >= axis.keys.len() {
                return None;
            }
            index = index * axis.keys.len() + coord;
        }
        Some(index)
    }
}

/// Keyforms of one element on a (possibly shared) binding.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyformGrid<T> {
    pub binding: BindingId,
    pub keyforms: Vec<T>,
}

impl<T> KeyformGrid<T> {
    pub fn new(binding: BindingId, keyforms: Vec<T>) -> Self {
        Self { binding, keyforms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::Arena;
    use crate::elements::Parameter;

    fn params() -> (ParameterId, ParameterId) {
        let mut arena = Arena::new();
        (
            arena.insert(Parameter::new(0.0, 1.0, 0.0)),
            arena.insert(Parameter::new(0.0, 1.0, 0.0)),
        )
    }

    fn axis(parameter: ParameterId, keys: &[f32]) -> Axis {
        Axis {
            parameter,
            keys: keys.to_vec(),
        }
    }

    #[test]
    fn count_is_product_of_key_counts() {
        let (a, b) = params();
        let binding = Binding::new(vec![axis(a, &[-1.0, 0.0, 1.0]), axis(b, &[0.0, 1.0])]).unwrap();
        assert_eq!(binding.keyform_count(), 6);
    }

    #[test]
    fn index_is_row_major() {
        let (a, b) = params();
        let binding = Binding::new(vec![axis(a, &[0.0, 1.0, 2.0]), axis(b, &[0.0, 1.0])]).unwrap();
        assert_eq!(binding.index(&[0, 0]), Some(0));
        assert_eq!(binding.index(&[0, 1]), Some(1));
        assert_eq!(binding.index(&[1, 0]), Some(2));
        assert_eq!(binding.index(&[2, 1]), Some(5));
        assert_eq!(binding.index(&[3, 0]), None);
        assert_eq!(binding.index(&[0]), None);
    }

    #[test]
    fn invalid_bindings_are_rejected() {
        let (a, b) = params();
        assert_eq!(Binding::new(vec![]), Err(BindingError::NoAxes));
        assert_eq!(
            Binding::new(vec![axis(a, &[])]),
            Err(BindingError::EmptyAxis { axis: 0 })
        );
        assert_eq!(
            Binding::new(vec![axis(a, &[1.0, 1.0])]),
            Err(BindingError::NotAscending { axis: 0 })
        );
        assert_eq!(
            Binding::new(vec![axis(a, &[f32::NAN])]),
            Err(BindingError::NonFinite { axis: 0 })
        );
        assert_eq!(
            Binding::new(vec![axis(b, &[0.0]), axis(b, &[1.0])]),
            Err(BindingError::DuplicateParameter { axis: 1 })
        );
    }
}
