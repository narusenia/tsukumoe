// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The evaluated model behind the viewport, without any GPU state: the model
//! compiled for evaluation, one frame of its pose, and the fit of the view.
//! Parameter values come from the view state, not from the model.

use tsukumoe_core::compiled::CompiledModel;
use tsukumoe_core::eval::Frame;
use tsukumoe_core::model::Model;

use crate::view::ViewState;

/// Used when the model draws nothing at its default pose.
const FALLBACK_VIEW: [f32; 4] = [-1.0, -1.0, 1.0, 1.0];
/// Space around the model, as a fraction of its size.
const MARGIN: f32 = 0.05;

/// The bounding box `[x0, y0, x1, y1]` of `x, y` pairs, or `None` when empty.
pub fn bounds(positions: &[f32]) -> Option<[f32; 4]> {
    let points = positions.as_chunks::<2>().0;
    let (first, rest) = points.split_first()?;
    let init = [first[0], first[1], first[0], first[1]];
    Some(rest.iter().fold(init, |b, p| {
        [
            b[0].min(p[0]),
            b[1].min(p[1]),
            b[2].max(p[0]),
            b[3].max(p[1]),
        ]
    }))
}

/// Grows `bounds` by a margin and widens one side so the view has the aspect
/// ratio of a `width` x `height` target, keeping the center.
pub fn fit_view(bounds: [f32; 4], width: u32, height: u32) -> [f32; 4] {
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    let cx = (bounds[0] + bounds[2]) / 2.0;
    let cy = (bounds[1] + bounds[3]) / 2.0;
    let w = ((bounds[2] - bounds[0]) * (1.0 + 2.0 * MARGIN)).max(1e-3);
    let h = ((bounds[3] - bounds[1]) * (1.0 + 2.0 * MARGIN)).max(1e-3);
    let (w, h) = if w / h < aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
}

pub struct Scene {
    pub model: CompiledModel<'static>,
    pub frame: Frame,
    bounds: [f32; 4],
}

impl Scene {
    pub fn new(model: &Model) -> Result<Self, String> {
        let model = model.compile().map_err(|e| format!("{e:?}"))?;
        let mut frame = Frame::new(&model);
        frame.evaluate(&model);
        let bounds = bounds(frame.draw_list(&model).positions()).unwrap_or(FALLBACK_VIEW);
        Ok(Self {
            model,
            frame,
            bounds,
        })
    }

    pub fn bounds(&self) -> [f32; 4] {
        self.bounds
    }

    /// Sets every parameter of the frame from the view state (the default when
    /// the view has no value).
    pub fn apply(&mut self, view: &ViewState) {
        for i in 0..self.model.parameters.len() {
            let default = self.model.parameters[i].default;
            self.frame.set_parameter(i, default);
        }
        for (name, value) in view.set_values() {
            if let Some(i) = self.model.parameter_index(name) {
                self.frame.set_parameter(i, value);
            }
        }
    }

    pub fn advance(&mut self, dt: f32) {
        self.frame.advance(&self.model, dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;

    #[test]
    fn bounds_and_fit_keep_the_aspect_and_the_center() {
        assert_eq!(bounds(&[]), None);
        assert_eq!(
            bounds(&[1.0, 5.0, -2.0, 3.0, 4.0, -1.0]),
            Some([-2.0, -1.0, 4.0, 5.0])
        );
        let b = [0.0, 0.0, 2.0, 1.0];
        for (w, h) in [(200, 100), (100, 200), (100, 100)] {
            let v = fit_view(b, w, h);
            assert!((((v[2] - v[0]) / (v[3] - v[1])) - w as f32 / h as f32).abs() < 1e-4);
            assert!(v[0] <= 0.0 && v[2] >= 2.0 && v[1] <= 0.0 && v[3] >= 1.0);
        }
        let v = fit_view([3.0, 3.0, 3.0, 3.0], 0, 0);
        assert!(v[2] > v[0] && v[3] > v[1]);
    }

    #[test]
    fn a_view_value_reaches_the_frame_and_missing_ones_use_the_default() {
        let mut m = Model::new();
        for (n, d) in [("A", 0.25), ("B", 0.75)] {
            m.apply(Command::AddParameter {
                name: n.into(),
                min: 0.0,
                max: 1.0,
                default: d,
                repeat: false,
            })
            .unwrap();
        }
        let mut scene = Scene::new(&m).unwrap();
        let mut view = ViewState::default();
        let p = m
            .parameters()
            .get(m.parameters().find("A").unwrap())
            .unwrap();
        view.set_param("A", p, 1.0);
        scene.apply(&view);
        let a = scene.model.parameter_index("A").unwrap();
        let b = scene.model.parameter_index("B").unwrap();
        assert_eq!(
            (
                scene.frame.parameters_mut()[a],
                scene.frame.parameters_mut()[b]
            ),
            (1.0, 0.75)
        );
        view.reset_params();
        scene.apply(&view);
        assert_eq!(scene.frame.parameters_mut()[a], 0.25);
    }
}
