// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The lipsync analyzer on synthetic speech.

mod common;

use common::{NAMES, VOWELS, vowel};
use tsukumoe_ai::{Lipsync, LipsyncConfig, MouthFrame};

fn run_chunks(sr: u32, cfg: &LipsyncConfig, x: &[f32], sizes: &[usize]) -> Vec<MouthFrame> {
    let mut l = Lipsync::new(sr, cfg.clone()).unwrap();
    let mut out = Vec::new();
    let (mut at, mut i) = (0, 0);
    while at < x.len() {
        let n = sizes[i % sizes.len()].min(x.len() - at);
        l.push(&x[at..at + n], |f| out.push(f));
        at += n;
        i += 1;
    }
    out
}

fn run(sr: u32, x: &[f32]) -> Vec<MouthFrame> {
    run_chunks(sr, &LipsyncConfig::default(), x, &[x.len().max(1)])
}

fn argmax(w: &[f32; 5]) -> usize {
    (0..5).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap()
}

fn noise(n: usize, amp: f32) -> Vec<f32> {
    let mut s = 12345u32;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((s >> 8) as f32 / 8_388_608.0 - 1.0) * amp
        })
        .collect()
}

#[test]
fn every_vowel_is_the_largest_weight_for_a_male_and_a_female_voice() {
    let mut failures = Vec::new();
    for sr in [16_000u32, 22_050, 44_100, 48_000] {
        for f0 in [120.0, 220.0] {
            for (v, t) in VOWELS.iter().enumerate() {
                let x = vowel(sr as f32, f0, *t, 0.6, 0.3);
                let frames = run(sr, &x);
                // Judge the steady part: after the first 150 ms.
                let steady = &frames[15..];
                let hits = steady.iter().filter(|f| argmax(&f.vowels) == v).count();
                let last = frames.last().unwrap();
                println!(
                    "sr {sr} f0 {f0} {}: {hits}/{} frames, last weights {:.2?}, open {:.2}",
                    NAMES[v],
                    steady.len(),
                    last.vowels,
                    last.open
                );
                if hits != steady.len() || last.vowels[v] < 0.5 {
                    failures.push(format!("sr {sr} f0 {f0} {}", NAMES[v]));
                }
            }
        }
    }
    assert!(failures.is_empty(), "misclassified: {failures:?}");
}

#[test]
fn silence_closes_the_mouth_and_leaves_the_vowel_alone() {
    let mut x = vowel(16_000.0, 120.0, VOWELS[1], 0.5, 0.3);
    let spoken = run(16_000, &x).len();
    x.extend(vec![0.0; 8_000]);
    let frames = run(16_000, &x);
    let tail = &frames[spoken + 4..]; // window fully silent
    let held = tail[0].vowels;
    assert_eq!(argmax(&held), 1);
    assert!(tail.iter().all(|f| f.vowels == held && !f.voiced));
    assert!(tail.last().unwrap().open < 0.01);

    let silent = run(16_000, &[0.0; 16_000]);
    assert!(silent.iter().all(|f| f.open == 0.0 && f.vowels == [0.0; 5]));
}

#[test]
fn louder_means_more_open() {
    for sr in [16_000u32, 48_000] {
        let mut last = -1.0;
        for amp in [0.002f32, 0.005, 0.01, 0.02, 0.05] {
            let open = run(sr, &vowel(sr as f32, 120.0, VOWELS[0], 0.6, amp))
                .last()
                .unwrap()
                .open;
            println!("sr {sr} amp {amp}: open {open:.3}");
            assert!(open > last, "amp {amp}: {open} <= {last}");
            last = open;
        }
        assert!(last < 1.0);
    }
}

#[test]
fn white_noise_does_not_move_the_vowel() {
    let mut x = vowel(16_000.0, 220.0, VOWELS[4], 0.4, 0.3);
    let spoken = run(16_000, &x).len();
    x.extend(noise(48_000, 0.3));
    let frames = run(16_000, &x);
    let held = frames[spoken - 1].vowels;
    let noisy = &frames[spoken + 4..];
    let voiced = noisy.iter().filter(|f| f.voiced).count();
    println!("noise: {voiced}/{} frames voiced", noisy.len());
    assert!(voiced * 20 <= noisy.len());
    assert!(noisy.iter().all(|f| argmax(&f.vowels) == 4));
    assert!(noisy.iter().all(|f| (0.0..=1.0).contains(&f.open)));
    // Noise alone never sets a vowel either.
    let only = run(16_000, &noise(48_000, 0.3));
    assert!(only.iter().filter(|f| f.vowels != [0.0; 5]).count() * 20 <= only.len());
    assert_eq!(argmax(&held), 4);
}

#[test]
fn latency_is_within_budget() {
    for sr in [16_000u32, 44_100, 48_000] {
        let l = Lipsync::new(sr, LipsyncConfig::default()).unwrap();
        assert!(l.latency().as_millis() <= 50, "{:?}", l.latency());
        assert!((l.hop_seconds() - 0.01).abs() < 1e-3);
        assert!((l.window_seconds() - 0.032).abs() < 2e-3);
    }
}

/// Random-sign samples of magnitude `a`: every full window has RMS exactly `a`.
fn signs(n: usize, a: f32) -> Vec<f32> {
    noise(n, 1.0).iter().map(|v| a.copysign(*v)).collect()
}

#[test]
fn rise_and_fall_follow_the_configured_time_constants() {
    // hop 10 ms, attack 15 ms, release 60 ms; a fixed floor keeps the target constant.
    let cfg = LipsyncConfig {
        floor_rise_db_per_s: 0.0,
        ..LipsyncConfig::default()
    };
    let loud = 0.1f32; // -20 dBFS
    let mut x = signs(16_000, loud);
    x.extend(vec![0.0; 16_000]);
    let frames = run_chunks(16_000, &cfg, &x, &[x.len()]);
    // Rise: the target is constant, so open_n = target * (1 - e^(-n * hop / attack)).
    let target = frames[60].open;
    assert!(target > 0.5 && target < 1.0, "{target}");
    let a = (-10.0f32 / cfg.attack_ms).exp();
    for (n, f) in frames.iter().take(6).enumerate() {
        let expect = target * (1.0 - a.powi(n as i32 + 1));
        assert!(
            (f.open - expect).abs() < 1e-3 * target.max(1.0) + 1e-3,
            "n {n}: {} vs {expect}",
            f.open
        );
    }
    // Fall: once the window holds only zeros the target is 0.
    let r = (-10.0f32 / cfg.release_ms).exp();
    let from = 100 + 4;
    for k in from..from + 10 {
        let ratio = frames[k + 1].open / frames[k].open;
        assert!((ratio - r).abs() < 1e-3, "frame {k}: {ratio} vs {r}");
    }
}

#[test]
fn splitting_the_input_never_changes_the_frames() {
    for sr in [16_000u32, 44_100] {
        let s = sr as f32;
        let mut x = vowel(s, 120.0, VOWELS[0], 0.25, 0.3);
        x.extend(vec![0.0; sr as usize / 10]);
        x.extend(vowel(s, 220.0, VOWELS[1], 0.25, 0.2));
        x.extend(noise(sr as usize / 5, 0.3));
        let cfg = LipsyncConfig::default();
        let whole = run_chunks(sr, &cfg, &x, &[x.len()]);
        assert!(whole.len() > 50);
        for sizes in [
            &[1usize][..],
            &[7, 1, 333, 2, 1000, 3],
            &[160],
            &[161],
            &[4096],
        ] {
            assert_eq!(run_chunks(sr, &cfg, &x, sizes), whole, "{sizes:?} at {sr}");
        }
    }
}

#[test]
fn bad_rates_settings_and_samples_are_handled() {
    let ok = LipsyncConfig::default();
    assert!(Lipsync::new(7_999, ok.clone()).is_err());
    assert!(Lipsync::new(192_001, ok.clone()).is_err());
    assert!(Lipsync::new(8_000, ok.clone()).is_ok());
    assert!(Lipsync::new(192_000, ok.clone()).is_ok());
    for bad in [
        LipsyncConfig {
            window_ms: f32::NAN,
            ..ok.clone()
        },
        LipsyncConfig {
            window_ms: 5.0,
            ..ok.clone()
        },
        LipsyncConfig {
            hop_ms: 0.0,
            ..ok.clone()
        },
        LipsyncConfig {
            hop_ms: 50.0,
            ..ok.clone()
        },
        LipsyncConfig {
            vowel_frames: 0,
            ..ok.clone()
        },
        LipsyncConfig {
            full_db: -80.0,
            ..ok.clone()
        },
        LipsyncConfig {
            formant_sigma: [0.0, 1.0],
            ..ok.clone()
        },
        LipsyncConfig {
            attack_ms: -1.0,
            ..ok.clone()
        },
    ] {
        assert!(Lipsync::new(16_000, bad).is_err());
    }
    let mut l = Lipsync::new(16_000, ok).unwrap();
    let mut n = 0;
    l.push(&[], |_| n += 1);
    let mut x = vowel(16_000.0, 120.0, VOWELS[0], 0.2, 0.3);
    x[100] = f32::NAN;
    x[200] = f32::INFINITY;
    x[300] = f32::NEG_INFINITY;
    x[400] = 1e30;
    l.push(&x, |f| {
        n += 1;
        assert!(f.open.is_finite() && f.vowels.iter().all(|v| v.is_finite()));
    });
    assert!(n > 0);
}

#[test]
fn json_overrides_settings_and_rejects_unknown_fields() {
    let c = LipsyncConfig::from_json(r#"{"hop_ms": 5, "formant_scale": [1.1, 0.9]}"#).unwrap();
    assert_eq!(c.hop_ms, 5.0);
    assert_eq!(c.window_ms, 32.0);
    assert_eq!(c.formant_scale, [1.1, 0.9]);
    assert!(LipsyncConfig::from_json(r#"{"hop": 5}"#).is_err());
    assert!(LipsyncConfig::from_json(r#"{"hop_ms": 500}"#).is_err());
    assert!(LipsyncConfig::from_json("nope").is_err());
}
