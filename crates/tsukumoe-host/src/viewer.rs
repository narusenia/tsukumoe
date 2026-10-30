// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::fs;
use std::path::Path;

use tsukumoe_core::compiled::CompiledModel;
use tsukumoe_core::eval::Frame;
use tsukumoe_format::{Project, read_model};
use tsukumoe_gpu::{Renderer, Texture};

/// Used when the model draws nothing at its default pose.
const FALLBACK_VIEW: [f32; 4] = [-1.0, -1.0, 1.0, 1.0];
/// Space around the model, as a fraction of its size.
const MARGIN: f32 = 0.05;

#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// Opens a project from a directory or, if it is not one, a zip archive.
pub fn open_project(path: &Path) -> Result<Project, String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    if path.is_dir() {
        Project::read_dir(path).map_err(|e| err(&e))
    } else {
        let bytes = fs::read(path).map_err(|e| err(&e))?;
        Project::read_zip(&bytes).map_err(|e| err(&e))
    }
}

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
    // A degenerate box (one point, or a line) still gets a visible size.
    let w = ((bounds[2] - bounds[0]) * (1.0 + 2.0 * MARGIN)).max(1e-3);
    let h = ((bounds[3] - bounds[1]) * (1.0 + 2.0 * MARGIN)).max(1e-3);
    let (w, h) = if w / h < aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
}

/// The model and its pose, without any GPU state.
pub struct Scene {
    model: CompiledModel,
    frame: Frame,
    params: Vec<ParamInfo>,
    values: Vec<f32>,
    bounds: [f32; 4],
}

impl Scene {
    pub fn new(project: &Project) -> Result<Self, String> {
        let model = read_model(project)
            .map_err(|e| e.to_string())?
            .compile()
            .map_err(|e| format!("{e:?}"))?;
        let params: Vec<ParamInfo> = model
            .parameter_names
            .iter()
            .zip(&model.parameters)
            .map(|(name, p)| ParamInfo {
                name: name.clone(),
                min: p.min,
                max: p.max,
                default: p.default,
            })
            .collect();
        let values = params.iter().map(|p| p.default).collect();
        let mut frame = Frame::new(&model);
        frame.evaluate(&model);
        let bounds = bounds(frame.draw_list(&model).positions()).unwrap_or(FALLBACK_VIEW);
        Ok(Self {
            model,
            frame,
            params,
            values,
            bounds,
        })
    }

    pub fn parameters(&self) -> &[ParamInfo] {
        &self.params
    }

    pub fn value(&self, index: usize) -> f32 {
        self.values[index]
    }

    /// Sets a parameter, clamped to its range.
    pub fn set_parameter(&mut self, index: usize, value: f32) {
        let p = &self.params[index];
        let value = value.clamp(p.min, p.max);
        self.values[index] = value;
        self.frame.set_parameter(index, value);
    }

    pub fn reset(&mut self) {
        for i in 0..self.params.len() {
            self.set_parameter(i, self.params[i].default);
        }
    }

    /// The bounding box of the default pose.
    pub fn bounds(&self) -> [f32; 4] {
        self.bounds
    }

    pub fn advance(&mut self, dt: f32) {
        self.frame.advance(&self.model, dt);
    }
}

/// A [`Scene`] and the renderer that draws it.
pub struct Viewer {
    pub scene: Scene,
    renderer: Renderer,
}

impl Viewer {
    /// Loads the project's textures (`textures/*.png`, in path order) into a
    /// renderer on `device`.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        project: &Project,
    ) -> Result<Self, String> {
        let scene = Scene::new(project)?;
        let textures = project
            .files
            .iter()
            .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
            .map(|(path, bytes)| Texture::from_png(bytes).map_err(|e| format!("{path}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let mut renderer = Renderer::new(device);
        renderer
            .set_textures(device, queue, &textures)
            .map_err(|e| e.to_string())?;
        Ok(Self { scene, renderer })
    }

    /// Advances the model by `dt` seconds and draws it, fitted to `size`, into
    /// `target`: an `Rgba8Unorm` render attachment of that size.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        size: (u32, u32),
        dt: f32,
    ) -> Result<(), String> {
        self.scene.advance(dt);
        let view = fit_view(self.scene.bounds, size.0, size.1);
        let list = self.scene.frame.draw_list(&self.scene.model);
        self.renderer
            .render_to_view(device, queue, &list, view, target, size)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Project {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tsukumoe-format/tests/fixtures/minimal");
        open_project(&dir).unwrap()
    }

    #[test]
    fn parameter_table_comes_from_the_model() {
        let scene = Scene::new(&fixture()).unwrap();
        let names: Vec<_> = scene.parameters().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["ParamAngle", "ParamEyeOpen"]);
        let eye = &scene.parameters()[1];
        assert_eq!((eye.min, eye.max, eye.default), (0.0, 1.0, 1.0));
        assert_eq!(scene.value(1), 1.0);
    }

    #[test]
    fn values_are_clamped_and_reset_restores_defaults() {
        let mut scene = Scene::new(&fixture()).unwrap();
        scene.set_parameter(0, 100.0);
        scene.set_parameter(1, 0.25);
        assert_eq!((scene.value(0), scene.value(1)), (30.0, 0.25));
        scene.reset();
        assert_eq!((scene.value(0), scene.value(1)), (0.0, 1.0));
    }

    #[test]
    fn a_model_with_nothing_to_draw_gets_the_fallback_view() {
        assert_eq!(Scene::new(&fixture()).unwrap().bounds(), FALLBACK_VIEW);
        assert_eq!(bounds(&[]), None);
    }

    #[test]
    fn bounds_cover_all_points() {
        assert_eq!(
            bounds(&[1.0, 5.0, -2.0, 3.0, 4.0, -1.0]),
            Some([-2.0, -1.0, 4.0, 5.0])
        );
    }

    #[test]
    fn fit_view_keeps_the_target_aspect_and_the_center() {
        let b = [0.0, 0.0, 2.0, 1.0];
        for (w, h) in [(200, 100), (100, 200), (100, 100)] {
            let v = fit_view(b, w, h);
            let aspect = (v[2] - v[0]) / (v[3] - v[1]);
            assert!((aspect - w as f32 / h as f32).abs() < 1e-4, "{w}x{h}");
            assert!(((v[0] + v[2]) / 2.0 - 1.0).abs() < 1e-5);
            assert!(((v[1] + v[3]) / 2.0 - 0.5).abs() < 1e-5);
            // The whole model stays inside the view.
            assert!(
                v[0] <= 0.0 && v[2] >= 2.0 && v[1] <= 0.0 && v[3] >= 1.0,
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn fit_view_survives_a_single_point_and_a_zero_size() {
        let v = fit_view([3.0, 3.0, 3.0, 3.0], 0, 0);
        assert!(v[2] > v[0] && v[3] > v[1]);
    }

    #[test]
    fn a_missing_project_is_an_error() {
        assert!(open_project(Path::new("/no/such/project")).is_err());
    }
}
