// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe lipsync`: a synthetic WAV ("あ", silence, "い") becomes a motion
//! file that the runtime loads and plays.

use std::f32::consts::TAU;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use tsukumoe_ai::LipsyncConfig;
use tsukumoe_ai::wav;
use tsukumoe_cli::lipsync;
use tsukumoe_core::elements::{Parameter, Part};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::tmod::PlaybackData;
use tsukumoe_format::dto::{CurveDto, MotionFile, SegmentDto};
use tsukumoe_format::{Manifest, Project, read_model, write_model};
use tsukumoe_runtime::{Priority, Runtime};

const RATE: u32 = 16_000;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tsukumoe"))
        .args(args)
        .output()
        .unwrap()
}

/// A vowel by a pulse train (120 Hz) through resonators at `formants`.
fn vowel(formants: [f32; 4], seconds: f32) -> Vec<f32> {
    let sr = RATE as f32;
    let mut state = [[0.0f32; 2]; 4];
    let coef: Vec<[f32; 3]> = formants
        .iter()
        .zip([80.0, 100.0, 120.0, 150.0])
        .map(|(&f, bw)| {
            let c = -(-TAU * bw / sr).exp();
            let b = 2.0 * (-TAU * bw / (2.0 * sr)).exp() * (TAU * f / sr).cos();
            [1.0 - b - c, b, c]
        })
        .collect();
    let (mut phase, mut tilt) = (0.0f32, 0.0f32);
    let mut out: Vec<f32> = (0..(seconds * sr) as usize)
        .map(|_| {
            phase += 120.0 / sr;
            let pulse = if phase >= 1.0 {
                phase -= 1.0;
                1.0
            } else {
                0.0
            };
            tilt = pulse + 0.9 * tilt;
            coef.iter().zip(&mut state).fold(tilt, |x, (k, s)| {
                let y = k[0] * x + k[1] * s[0] + k[2] * s[1];
                *s = [y, s[0]];
                y
            })
        })
        .collect();
    let peak = out.iter().fold(1e-9f32, |m, v| m.max(v.abs()));
    out.iter_mut().for_each(|v| *v *= 0.3 / peak);
    out
}

/// 16-bit mono WAV bytes.
fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut out = b"RIFF".to_vec();
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend([1, 0, 1, 0]);
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend([2, 0, 16, 0]);
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// あ (0 to 0.5 s), silence (to 1.0 s), い (to 1.5 s).
fn speech() -> Vec<f32> {
    let mut x = vowel([800.0, 1200.0, 2800.0, 3500.0], 0.5);
    x.extend(vec![0.0; RATE as usize / 2]);
    x.extend(vowel([300.0, 2300.0, 2800.0, 3500.0], 0.5));
    x
}

/// The value of a piecewise-linear curve at `t`.
fn value(c: &CurveDto, t: f32) -> f32 {
    let (mut t0, mut v0) = (c.start[0], c.start[1]);
    for s in &c.segments {
        let SegmentDto::Linear { point } = s else {
            panic!("only linear segments are written");
        };
        if t < point[0] {
            return v0 + (point[1] - v0) * (t - t0) / (point[0] - t0);
        }
        (t0, v0) = (point[0], point[1]);
    }
    v0
}

fn curve<'a>(m: &'a MotionFile, name: &str) -> &'a CurveDto {
    m.curves
        .iter()
        .find(|c| matches!(&c.target, tsukumoe_format::dto::CurveTargetDto::Parameter { name: n } if n == name))
        .unwrap()
}

fn export(tolerance: f32) -> MotionFile {
    let audio = wav::parse(&wav_bytes(&speech())).unwrap();
    lipsync::motion(&audio, LipsyncConfig::default(), 30.0, tolerance, "lipsync").unwrap()
}

#[test]
fn the_curves_follow_the_sections() {
    let m = export(0.01);
    assert_eq!((m.fps, m.name.as_str(), m.looped), (30.0, "lipsync", false));
    assert!((m.duration - 1.5).abs() < 1e-3);
    let (y, form) = (curve(&m, "ParamMouthOpenY"), curve(&m, "ParamMouthForm"));
    // あ: wide open, neutral form.
    assert!(value(y, 0.35) > 0.75, "{}", value(y, 0.35));
    assert!(value(form, 0.35).abs() < 0.2, "{}", value(form, 0.35));
    // Silence: closed.
    assert!(value(y, 0.85) < 0.05 && value(form, 0.85).abs() < 0.1);
    // い: half open, spread.
    let (oy, f) = (value(y, 1.35), value(form, 1.35));
    assert!((0.2..0.55).contains(&oy) && f > 0.5, "{oy} {f}");
}

#[test]
fn thinning_stays_within_the_tolerance_and_drops_points() {
    let dense = export(0.0);
    let thin = export(0.02);
    let count = |m: &MotionFile| m.curves.iter().map(|c| c.segments.len()).sum::<usize>();
    assert!(
        count(&dense) >= 45 && count(&thin) < count(&dense) / 2,
        "{} {}",
        count(&dense),
        count(&thin)
    );
    for name in ["ParamMouthOpenY", "ParamMouthForm"] {
        for j in 0..=45 {
            let t = j as f32 / 30.0;
            let err = (value(curve(&dense, name), t) - value(curve(&thin, name), t)).abs();
            assert!(err <= 0.02 + 1e-5, "{name} at {t}: {err}");
        }
    }
}

#[test]
fn the_command_writes_a_motion_the_runtime_plays() {
    let dir = scratch("lipsync");
    let (input, output) = (dir.join("speech.wav"), dir.join("mouth.json"));
    fs::write(&input, wav_bytes(&speech())).unwrap();
    let out = run(&[
        "lipsync",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--name",
        "talk",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let file: MotionFile = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(file.name, "talk");

    // A model with the two standard mouth parameters, plus the motion file.
    let mut p = ModelParts::default();
    p.parts.insert("Part", Part::default()).unwrap();
    p.parameters
        .insert("ParamMouthOpenY", Parameter::new(0.0, 1.0, 0.0))
        .unwrap();
    p.parameters
        .insert("ParamMouthForm", Parameter::new(-1.0, 1.0, 0.0))
        .unwrap();
    let mut project = Project::new(Manifest::new(Some("test".into())));
    write_model(&Model::from_parts(p).unwrap(), &mut project).unwrap();
    project
        .files
        .insert("motions/talk.json".into(), fs::read(&output).unwrap());
    let model = read_model(&project).unwrap();
    let mut rt = Runtime::new(
        model.compile().unwrap(),
        PlaybackData::from_model(&model).unwrap(),
    )
    .unwrap();
    let (iy, iform) = (
        rt.parameter_index("ParamMouthOpenY").unwrap(),
        rt.parameter_index("ParamMouthForm").unwrap(),
    );
    assert!(rt.play_motion("talk", 0, Priority::Normal).unwrap());
    // Runtime time 0.3 s (あ), 0.8 s (silence), 1.3 s (い).
    let mut seen = Vec::new();
    for step in 1..=78 {
        rt.update(1.0 / 60.0);
        if matches!(step, 18 | 48 | 78) {
            seen.push((rt.parameter_values()[iy], rt.parameter_values()[iform]));
        }
    }
    assert!(seen[0].0 > 0.75 && seen[0].1.abs() < 0.2, "{seen:?}");
    assert!(seen[1].0 < 0.1 && seen[1].1.abs() < 0.1, "{seen:?}");
    assert!(
        (0.2..0.55).contains(&seen[2].0) && seen[2].1 > 0.5,
        "{seen:?}"
    );
    // Check the value mid-あ against the file.
    let mut rt2 = Runtime::new(
        model.compile().unwrap(),
        PlaybackData::from_model(&model).unwrap(),
    )
    .unwrap();
    rt2.play_motion("talk", 0, Priority::Normal).unwrap();
    for _ in 0..21 {
        rt2.update(1.0 / 60.0);
    }
    let expect = value(curve(&file, "ParamMouthOpenY"), 21.0 / 60.0);
    assert!(expect > 0.7);
    assert!(
        (rt2.parameter_values()[iy] - expect).abs() < 0.02,
        "{} vs {expect}",
        rt2.parameter_values()[iy]
    );
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    let dir = scratch("lipsync-bad");
    let out = dir.join("out.json");
    let bad = dir.join("bad.wav");
    fs::write(&bad, b"RIFF....WAVEjunk").unwrap();
    for args in [
        vec![
            "lipsync",
            bad.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ],
        vec!["lipsync", "/nonexistent.wav", "-o", out.to_str().unwrap()],
    ] {
        assert_eq!(run(&args).status.code(), Some(1));
    }
    assert_eq!(
        run(&["lipsync", bad.to_str().unwrap()]).status.code(),
        Some(2)
    );
    let good = dir.join("good.wav");
    fs::write(&good, wav_bytes(&[0.0; 3200])).unwrap();
    for opt in [["--fps", "0"], ["--fps", "nan"], ["--tolerance", "-1"]] {
        let code = run(&[
            "lipsync",
            good.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            opt[0],
            opt[1],
        ])
        .status
        .code();
        assert!(matches!(code, Some(1 | 2)), "{opt:?}: {code:?}");
    }
    assert!(!out.exists());
}
