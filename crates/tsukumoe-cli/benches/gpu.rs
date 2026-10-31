// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! GPU benchmarks on the reference model (REQ-PERF-004 / REQ-PERF-006). They
//! need a hardware adapter and skip without one. Medians are written where
//! `scripts/check-bench.sh` reads criterion results.
//!
//! - `gpu_reference_render_1080p`: the wait for the GPU after submitting one
//!   model at 1920x1080 (an upper bound of its GPU time; Metal's timestamp
//!   queries are not usable, see `Headless::wait`).
//! - `frame_reference_x50_1080p` / `_p99`: wall time of one frame of 50
//!   models, from `advance` (drivers and physics included) to the GPU being
//!   done with the 1080p image. The models are evaluated one by one.
//! - `frame_reference_x50_1080p_parallel` / `_p99`: the same, with the
//!   evaluation spread over `min(cores, MAX_THREADS)` threads. Drawing and
//!   submitting stay on one thread.

use std::time::{Duration, Instant};

use tsukumoe_cli::generate::{Table, project};
use tsukumoe_core::eval::Frame;
use tsukumoe_gpu::{Headless, Texture};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const MODELS: usize = 50;
const COLUMNS: usize = 10;
const ROWS: usize = MODELS / COLUMNS;
/// The most threads the parallel frame uses; more only adds start-up cost
/// for 50 models of 0.15 ms each.
const MAX_THREADS: usize = 8;

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// Records `d` as a criterion result so `check-bench.sh` can read it.
fn record(name: &str, d: Duration) {
    let dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| format!("{}/../../target", env!("CARGO_MANIFEST_DIR")));
    let dir = format!("{dir}/criterion/{name}/new");
    std::fs::create_dir_all(&dir).expect("create result dir");
    let json = format!("{{\"median\":{{\"point_estimate\":{}}}}}", d.as_nanos());
    std::fs::write(format!("{dir}/estimates.json"), json).expect("write result");
}

fn main() {
    // `cargo test` runs benches with --bench absent; do nothing then.
    if !std::env::args().any(|a| a == "--bench") {
        return;
    }
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let (project, model) = project(&table).unwrap();
    let compiled = model.compile().unwrap();
    let textures: Vec<Texture> = project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
        .map(|(_, bytes)| Texture::from_png(bytes).unwrap())
        .collect();
    let Ok(mut gpu) = Headless::hardware() else {
        eprintln!("skipped: no hardware adapter");
        return;
    };
    gpu.set_textures(&textures).unwrap();
    println!("adapter: {}", gpu.adapter());

    // The pose: every parameter at the middle of a sweep, so the meshes move.
    let pose = |frame: &mut Frame, t: f32| {
        for (i, v) in frame.parameters_mut().iter_mut().enumerate() {
            *v = if i % 2 == 0 { t } else { 1.0 - t };
        }
    };
    let mut one = Frame::new(&compiled);
    pose(&mut one, 0.5);
    one.evaluate(&compiled);
    let b = {
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for p in one.draw_list(&compiled).positions().as_chunks::<2>().0 {
            b = [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ];
        }
        b
    };

    // PERF-004: one model filling the frame (the 16:9 view is wider than the model).
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    let side = h * WIDTH as f32 / HEIGHT as f32;
    let view = [b[0] - (side - w) / 2.0, b[1], b[0] + (side + w) / 2.0, b[3]];
    let mut times = Vec::new();
    for i in 0..220 {
        let list = one.draw_list(&compiled);
        gpu.draw(&list, view, true, WIDTH, HEIGHT).unwrap();
        let t = gpu.wait().unwrap();
        if i >= 20 {
            times.push(t);
        }
    }
    times.sort();
    let median = percentile(&times, 0.5);
    println!(
        "PERF-004 GPU wait 1080p: median {:.3} ms, p99 {:.3} ms, min {:.3} ms ({} runs)",
        ms(median),
        ms(percentile(&times, 0.99)),
        ms(times[0]),
        times.len()
    );
    record("gpu_reference_render_1080p", median);

    // PERF-006: 50 models in a grid of cells, each with its own phase.
    let mut frames: Vec<Frame> = (0..MODELS).map(|_| Frame::new(&compiled)).collect();
    let (cw, ch) = (w * 1.02, h * 1.02);
    let views: Vec<[f32; 4]> = (0..MODELS)
        .map(|i| {
            let (x, y) = ((i % COLUMNS) as f32 * cw, (i / COLUMNS) as f32 * ch);
            [
                b[0] - x,
                b[1] - y,
                b[0] - x + COLUMNS as f32 * cw,
                b[1] - y + ROWS as f32 * ch,
            ]
        })
        .collect();
    // One frame per call: every model is evaluated (on `threads` threads, or
    // one by one with its draw submitted right away when `None`), then the
    // models are drawn on this thread, which is the only one that submits.
    let mut t = 0.0_f32;
    let mut run = |threads: Option<usize>, gpu: &mut Headless| {
        let (mut total, mut eval, mut wait) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..330 {
            let start = Instant::now();
            t = (t + 0.013) % 1.0;
            let mut update = Duration::ZERO;
            let phase = |k: usize| (t + k as f32 / MODELS as f32) % 1.0;
            match threads {
                None => {
                    for (k, f) in frames.iter_mut().enumerate() {
                        // The GPU draws one model while the CPU evaluates the next.
                        let began = Instant::now();
                        pose(f, phase(k));
                        f.advance(&compiled, 1.0 / 60.0);
                        update += began.elapsed();
                        gpu.draw(&f.draw_list(&compiled), views[k], k == 0, WIDTH, HEIGHT)
                            .unwrap();
                    }
                }
                Some(threads) => {
                    let per = MODELS.div_ceil(threads);
                    std::thread::scope(|scope| {
                        for (c, chunk) in frames.chunks_mut(per).enumerate() {
                            let (compiled, pose) = (&compiled, &pose);
                            scope.spawn(move || {
                                for (j, f) in chunk.iter_mut().enumerate() {
                                    pose(f, phase(c * per + j));
                                    f.advance(compiled, 1.0 / 60.0);
                                }
                            });
                        }
                    });
                    update = start.elapsed();
                    for (k, f) in frames.iter().enumerate() {
                        gpu.draw(&f.draw_list(&compiled), views[k], k == 0, WIDTH, HEIGHT)
                            .unwrap();
                    }
                }
            }
            let waited = gpu.wait().unwrap();
            if i >= 30 {
                total.push(start.elapsed());
                eval.push(update);
                wait.push(waited);
            }
        }
        for v in [&mut total, &mut eval, &mut wait] {
            v.sort();
        }
        (total, eval, wait)
    };
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let threads = cores.min(MAX_THREADS);
    for (label, mode, name) in [
        ("1 thread", None, "frame_reference_x50_1080p"),
        (
            &*format!("{threads} threads of {cores} cores"),
            Some(threads),
            "frame_reference_x50_1080p_parallel",
        ),
    ] {
        let (total, eval, wait) = run(mode, &mut gpu);
        println!(
            "PERF-006 frame of {MODELS} models at 1080p, update on {label}: median {:.3} ms, p99 {:.3} ms (update {:.3} ms, final GPU wait {:.3} ms median; {} frames)",
            ms(percentile(&total, 0.5)),
            ms(percentile(&total, 0.99)),
            ms(percentile(&eval, 0.5)),
            ms(percentile(&wait, 0.5)),
            total.len()
        );
        record(name, percentile(&total, 0.5));
        record(&format!("{name}_p99"), percentile(&total, 0.99));
    }
}
