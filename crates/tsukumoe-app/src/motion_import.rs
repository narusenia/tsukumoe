// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Importing a motion file (`motions/<name>.json`, what `tsukumoe lipsync`
//! writes) as a new motion of the open model. Curves whose parameter or part
//! the model lacks are left out and counted.

use tsukumoe_core::command::{Command, MotionTarget};
use tsukumoe_core::model::Model;
use tsukumoe_core::playback::Segment;
use tsukumoe_format::dto::{CurveTargetDto, MotionFile, SegmentDto};

/// What importing a file does.
pub struct Import {
    /// One undo step: the motion with its curves.
    pub command: Command,
    /// The name the motion got (the file's, made unique).
    pub name: String,
    pub curves: usize,
    /// Curves left out: unknown target, or a target already taken in the file.
    pub skipped: usize,
}

fn unique(model: &Model, base: &str) -> String {
    let base = if base.is_empty() { "Motion" } else { base };
    std::iter::once(base.to_owned())
        .chain((2..).map(|n| format!("{base}{n}")))
        .find(|n| model.motions().find(n).is_none())
        .expect("endless range")
}

pub fn parse(text: &str) -> Result<MotionFile, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

pub fn commands(model: &Model, file: &MotionFile) -> Import {
    let name = unique(model, file.name.trim());
    let mut commands = vec![
        Command::AddMotion {
            name: name.clone(),
            duration: file.duration,
            fps: file.fps,
            looped: file.looped,
        },
        Command::SetMotionTiming {
            name: name.clone(),
            duration: file.duration,
            fps: file.fps,
            looped: file.looped,
            fade_in: file.fade_in,
            fade_out: file.fade_out,
        },
    ];
    let mut taken: Vec<&CurveTargetDto> = Vec::new();
    let (mut curves, mut skipped) = (0, 0);
    for c in &file.curves {
        let known = match &c.target {
            CurveTargetDto::Parameter { name } => model.parameters().find(name).is_some(),
            CurveTargetDto::PartOpacity { name } => model.parts().find(name).is_some(),
        };
        if !known || taken.contains(&&c.target) {
            skipped += 1;
            continue;
        }
        taken.push(&c.target);
        let target = match &c.target {
            CurveTargetDto::Parameter { name } => MotionTarget::Parameter { name: name.clone() },
            CurveTargetDto::PartOpacity { name } => {
                MotionTarget::PartOpacity { name: name.clone() }
            }
        };
        commands.push(Command::AddMotionCurve {
            motion: name.clone(),
            target,
        });
        commands.push(Command::SetMotionCurve {
            motion: name.clone(),
            curve: curves,
            start: c.start,
            segments: c
                .segments
                .iter()
                .map(|s| match *s {
                    SegmentDto::Linear { point } => Segment::Linear(point),
                    SegmentDto::Bezier { points } => Segment::Bezier(points),
                    SegmentDto::Stepped { point } => Segment::Stepped(point),
                    SegmentDto::InverseStepped { point } => Segment::InverseStepped(point),
                })
                .collect(),
        });
        if c.fade_in.is_some() || c.fade_out.is_some() {
            commands.push(Command::SetMotionCurveFade {
                motion: name.clone(),
                curve: curves,
                fade_in: c.fade_in,
                fade_out: c.fade_out,
            });
        }
        curves += 1;
    }
    Import {
        command: Command::Batch { commands },
        name,
        curves,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        let mut m = Model::new();
        m.apply(Command::AddParameter {
            name: "ParamMouthOpenY".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        m
    }

    const FILE: &str = r#"{"name":"talk","duration":1.0,"fps":30,"curves":[
        {"target":{"type":"parameter","name":"ParamMouthOpenY"},"start":[0,0],
         "segments":[{"type":"linear","point":[0.5,1]},{"type":"linear","point":[1,0]}]},
        {"target":{"type":"parameter","name":"ParamMouthOpenY"},"start":[0,0],"segments":[]},
        {"target":{"type":"parameter","name":"Gone"},"start":[0,0],"segments":[]}]}"#;

    #[test]
    fn imports_known_curves_and_counts_the_rest() {
        let mut m = model();
        let import = commands(&m, &parse(FILE).unwrap());
        assert_eq!((import.curves, import.skipped), (1, 2));
        m.apply(import.command).unwrap();
        let id = m.motions().find("talk").unwrap();
        assert_eq!(m.motions().get(id).unwrap().curves[0].key_count(), 3);
        // A second import does not collide.
        assert_eq!(commands(&m, &parse(FILE).unwrap()).name, "talk2");
        assert!(m.undo());
        assert!(m.motions().find("talk").is_none());
    }

    #[test]
    fn a_file_that_is_not_a_motion_is_an_error() {
        assert!(parse("{}").is_err());
    }
}
