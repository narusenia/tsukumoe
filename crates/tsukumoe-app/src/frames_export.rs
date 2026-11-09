// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Writing a motion out as a PNG sequence, and as a video when `ffmpeg` is on
//! the path (M5-09a). Each frame is rendered by the headless renderer, the way
//! `tsukumoe render` does; the camera is the box that holds every frame, so the
//! picture does not jump.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tsukumoe_core::eval::Frame;
use tsukumoe_core::model::Model;
use tsukumoe_core::playback::{Curve, CurveTarget, Motion};
use tsukumoe_format::Project;
use tsukumoe_gpu::{Headless, Texture};

/// The longer side of the pictures, in pixels.
pub const LONG_SIDE: u32 = 1024;

/// How many frames a motion has at its own rate: one per tick, and the last
/// tick too when the motion does not loop.
pub fn frame_count(motion: &Motion) -> usize {
    let ticks = (motion.duration * motion.fps).floor() as usize;
    if motion.looped {
        ticks.max(1)
    } else {
        ticks + 1
    }
}

/// The file name of frame `index` (from 0): `Idle_0001.png`.
pub fn frame_name(motion: &str, index: usize) -> String {
    format!("{}_{:04}.png", safe(motion), index + 1)
}

fn safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The size that keeps the aspect of `view` with `LONG_SIDE` on its longer side.
fn size_of(view: [f32; 4]) -> (u32, u32) {
    let (w, h) = (view[2] - view[0], view[3] - view[1]);
    let scale = LONG_SIDE as f32 / w.max(h);
    (
        ((w * scale).round() as u32).max(1),
        ((h * scale).round() as u32).max(1),
    )
}

/// The parameter curves of the motion, with the parameter's index in the model.
fn curves<'m>(
    model: &Model,
    compiled: &tsukumoe_core::compiled::CompiledModel,
    motion: &'m Motion,
) -> Vec<(usize, &'m Curve)> {
    motion
        .curves
        .iter()
        .filter_map(|c| match c.target {
            CurveTarget::Parameter(id) => {
                let index = compiled.parameter_index(model.parameters().name(id)?)?;
                Some((index, c))
            }
            CurveTarget::PartOpacity(_) => None,
        })
        .collect()
}

/// Renders every frame of `motion` and hands each PNG to `on_frame` (with its
/// index). Returns how many frames there were.
pub fn render(
    model: &Model,
    project: &Project,
    motion: &Motion,
    mut on_frame: impl FnMut(usize, Vec<u8>) -> Result<(), String>,
) -> Result<usize, String> {
    let compiled = model.compile().map_err(|e| format!("{e:?}"))?;
    let curves = curves(model, &compiled, motion);
    let count = frame_count(motion);
    let dt = 1.0 / motion.fps;
    // The same run twice: the first finds the box, the second draws.
    let run = |each: &mut dyn FnMut(usize, &mut Frame)| {
        let mut frame = Frame::new(&compiled);
        for k in 0..count {
            let t = k as f32 * dt;
            for (index, curve) in &curves {
                frame.set_parameter(*index, curve.value_at(t));
            }
            frame.advance(&compiled, dt);
            each(k, &mut frame);
        }
    };
    let mut view = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    run(&mut |_, frame| {
        for p in frame.draw_list(&compiled).positions().as_chunks::<2>().0 {
            view = [
                view[0].min(p[0]),
                view[1].min(p[1]),
                view[2].max(p[0]),
                view[3].max(p[1]),
            ];
        }
    });
    if !(view[2] > view[0] && view[3] > view[1]) {
        return Err("the model has nothing to draw".into());
    }
    let (width, height) = size_of(view);
    let textures = project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
        .map(|(path, bytes)| Texture::from_png(bytes).map_err(|e| format!("{path}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut gpu = Headless::new().map_err(|e| e.to_string())?;
    gpu.set_textures(&textures).map_err(|e| e.to_string())?;
    let mut failure = None;
    run(&mut |k, frame| {
        if failure.is_some() {
            return;
        }
        let list = frame.draw_list(&compiled);
        failure = gpu
            .render(&list, view, width, height)
            .map_err(|e| e.to_string())
            .and_then(|image| on_frame(k, image.to_png()))
            .err();
    });
    failure.map_or(Ok(count), Err)
}

/// Whether `ffmpeg` runs.
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The arguments that turn `<dir>/<motion>_%04d.png` into `<motion>.mp4`.
fn ffmpeg_args(dir: &Path, motion: &str, fps: f32) -> Vec<String> {
    let name = safe(motion);
    let pattern = dir.join(format!("{name}_%04d.png"));
    vec![
        "-y".into(),
        "-framerate".into(),
        fps.to_string(),
        "-i".into(),
        pattern.display().to_string(),
        // H.264 wants even sides.
        "-vf".into(),
        "pad=ceil(iw/2)*2:ceil(ih/2)*2".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        dir.join(format!("{name}.mp4")).display().to_string(),
    ]
}

/// Encodes the written frames into `<dir>/<motion>.mp4`.
pub fn encode(dir: &Path, motion: &str, fps: f32) -> Result<PathBuf, String> {
    let output = Command::new("ffmpeg")
        .args(ffmpeg_args(dir, motion, fps))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !output.status.success() {
        let log = String::from_utf8_lossy(&output.stderr);
        let last = log.lines().last().unwrap_or("ffmpeg failed");
        return Err(format!("ffmpeg: {last}"));
    }
    Ok(dir.join(format!("{}.mp4", safe(motion))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion(duration: f32, fps: f32, looped: bool) -> Motion {
        Motion {
            duration,
            fps,
            looped,
            fade_in: 0.0,
            fade_out: 0.0,
            curves: Vec::new(),
        }
    }

    #[test]
    fn a_motion_that_does_not_loop_has_its_last_tick_too() {
        assert_eq!(frame_count(&motion(2.0, 30.0, false)), 61);
        assert_eq!(frame_count(&motion(2.0, 30.0, true)), 60);
        assert_eq!(frame_count(&motion(0.0, 30.0, true)), 1);
        assert_eq!(frame_count(&motion(0.0, 30.0, false)), 1);
    }

    #[test]
    fn frames_are_numbered_from_one_and_names_are_safe() {
        assert_eq!(frame_name("Idle", 0), "Idle_0001.png");
        assert_eq!(frame_name("a/b c", 11), "a_b_c_0012.png");
    }

    #[test]
    fn the_long_side_is_fixed_and_the_aspect_kept() {
        assert_eq!(size_of([0.0, 0.0, 2.0, 1.0]), (LONG_SIDE, LONG_SIDE / 2));
        assert_eq!(size_of([0.0, 0.0, 1.0, 4.0]), (LONG_SIDE / 4, LONG_SIDE));
    }

    #[test]
    fn ffmpeg_reads_the_numbered_frames_and_writes_beside_them() {
        let args = ffmpeg_args(Path::new("/out"), "Idle", 30.0);
        assert!(args.contains(&"/out/Idle_%04d.png".to_owned()));
        assert_eq!(args.last().unwrap(), "/out/Idle.mp4");
        assert!(args.windows(2).any(|w| w == ["-framerate", "30"]));
    }

    #[test]
    fn every_frame_of_a_motion_is_rendered_as_a_png() {
        use tsukumoe_cli::generate::{Table, project};
        use tsukumoe_core::command::{Command, MotionTarget};

        let table = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/reference-model/parts.json"
        );
        let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
        let (project, mut model) = project(&table).unwrap();
        let param = model.parameters().iter().next().unwrap().1.to_owned();
        for c in [
            Command::AddMotion {
                name: "Wave".into(),
                duration: 0.2,
                fps: 10.0,
                looped: false,
            },
            Command::AddMotionCurve {
                motion: "Wave".into(),
                target: MotionTarget::Parameter { name: param },
            },
        ] {
            model.apply(c).unwrap();
        }
        let wave = model
            .motions()
            .get(model.motions().find("Wave").unwrap())
            .unwrap()
            .clone();
        let mut frames = Vec::new();
        let count = render(&model, &project, &wave, |k, png| {
            frames.push((k, png));
            Ok(())
        })
        .unwrap();
        assert_eq!(count, 3);
        assert_eq!(
            frames.iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(frames.iter().all(|f| f.1.starts_with(b"\x89PNG")));
        // With ffmpeg at hand the frames become a video.
        if ffmpeg_available() {
            let dir = std::env::temp_dir().join(format!("tsukumoe-frames-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            for (k, png) in &frames {
                std::fs::write(dir.join(frame_name("Wave", *k)), png).unwrap();
            }
            let video = encode(&dir, "Wave", wave.fps).unwrap();
            assert!(std::fs::metadata(&video).unwrap().len() > 0);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
