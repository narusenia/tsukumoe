// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Streaming lipsync analysis: PCM in, one [`MouthFrame`] per hop out.
//!
//! Loudness gives the mouth opening, LPC formants (F1, F2) of voiced frames
//! give the weights of the five Japanese vowels, and both are smoothed over
//! time. Pure signal processing, deterministic, no allocation per hop.

use std::f32::consts::TAU;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AiError;
use crate::formant::FormantFinder;

const PRE_EMPHASIS: f32 = 0.97;
/// Fundamental frequency range searched for the voiced decision.
const F0_RANGE: [f32; 2] = [70.0, 400.0];
/// Above this rate the input is decimated (by averaging) for the formants.
const ANALYSIS_RATE: f32 = 16_000.0;
/// The noise floor never rises above this.
const MAX_FLOOR_DB: f32 = -40.0;
/// `open` is scaled by this when a voiced frame has no usable formants.
const UNSURE_OPEN: f32 = 0.5;

/// あ い う え お.
pub const VOWEL_COUNT: usize = 5;

/// Analyzer settings. JSON can override any field; the rest stay default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LipsyncConfig {
    pub window_ms: f32,
    pub hop_ms: f32,
    /// RMS level (dBFS) below which the mouth is closed.
    pub silence_db: f32,
    /// RMS level (dBFS) at which the mouth is fully open.
    pub full_db: f32,
    /// How far above the tracked noise floor the opening starts.
    pub floor_margin_db: f32,
    /// How fast the noise floor may rise during sustained sound.
    pub floor_rise_db_per_s: f32,
    /// Time constants of the opening smoothing (rise, fall).
    pub attack_ms: f32,
    pub release_ms: f32,
    /// Moving-average length (in hops) of the vowel weights.
    pub vowel_frames: usize,
    /// Normalized autocorrelation peak above which a frame is voiced.
    pub voiced_threshold: f32,
    /// F1 and F2 (Hz) of あ い う え お.
    pub vowels: [[f32; 2]; VOWEL_COUNT],
    /// Per-speaker multipliers on the measured F1 and F2.
    pub formant_scale: [f32; 2],
    /// Distance scale (Hz) of F1 and F2 for the vowel weights.
    pub formant_sigma: [f32; 2],
}

impl Default for LipsyncConfig {
    fn default() -> Self {
        Self {
            window_ms: 32.0,
            hop_ms: 10.0,
            silence_db: -60.0,
            full_db: -18.0,
            floor_margin_db: 6.0,
            floor_rise_db_per_s: 0.5,
            attack_ms: 15.0,
            release_ms: 60.0,
            vowel_frames: 3,
            voiced_threshold: 0.35,
            vowels: [
                [800.0, 1200.0],
                [300.0, 2300.0],
                [350.0, 1300.0],
                [500.0, 1900.0],
                [500.0, 900.0],
            ],
            formant_scale: [1.0, 1.0],
            formant_sigma: [150.0, 350.0],
        }
    }
}

impl LipsyncConfig {
    pub fn from_json(text: &str) -> Result<Self, AiError> {
        let c: Self = serde_json::from_str(text).map_err(|e| AiError::Lipsync(e.to_string()))?;
        c.validate()?;
        Ok(c)
    }

    fn validate(&self) -> Result<(), AiError> {
        let numbers = [
            self.window_ms,
            self.hop_ms,
            self.silence_db,
            self.full_db,
            self.floor_margin_db,
            self.floor_rise_db_per_s,
            self.attack_ms,
            self.release_ms,
            self.voiced_threshold,
        ];
        let all = numbers
            .iter()
            .chain(self.vowels.iter().flatten())
            .chain(&self.formant_scale)
            .chain(&self.formant_sigma);
        let bad = |m: &str| Err(AiError::Lipsync(m.to_owned()));
        if all.into_iter().any(|v| !v.is_finite()) {
            return bad("settings must be finite numbers");
        }
        if !(10.0..=200.0).contains(&self.window_ms)
            || !(1.0..=self.window_ms).contains(&self.hop_ms)
        {
            return bad("need 10 <= window_ms <= 200 and 1 <= hop_ms <= window_ms");
        }
        if self.full_db <= self.silence_db {
            return bad("full_db must be above silence_db");
        }
        if self.attack_ms < 0.0 || self.release_ms < 0.0 || self.floor_rise_db_per_s < 0.0 {
            return bad("time constants and rates must not be negative");
        }
        if !(1..=64).contains(&self.vowel_frames) {
            return bad("vowel_frames must be 1..=64");
        }
        if self.formant_scale.iter().any(|&s| s <= 0.0)
            || self.formant_sigma.iter().any(|&s| s <= 0.0)
        {
            return bad("formant_scale and formant_sigma must be positive");
        }
        Ok(())
    }
}

/// The mouth at one hop.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MouthFrame {
    /// Opening, 0..1.
    pub open: f32,
    /// Weights of あ い う え お, summing to 1 once a vowel was heard, else 0.
    pub vowels: [f32; VOWEL_COUNT],
    /// Whether this hop was a voiced frame.
    pub voiced: bool,
}

/// One-pole smoothing coefficient for a time constant.
fn coefficient(tau_ms: f32, hop_s: f32) -> f32 {
    if tau_ms <= 0.0 {
        1.0
    } else {
        1.0 - (-hop_s * 1000.0 / tau_ms).exp()
    }
}

pub struct Lipsync {
    cfg: LipsyncConfig,
    decimation: usize,
    acc: f32,
    acc_n: usize,
    win: usize,
    hop: usize,
    /// Pitch lag search range, in analysis samples.
    lags: [usize; 2],
    hop_s: f32,
    attack: f32,
    release: f32,
    ring: Vec<f32>,
    head: usize,
    total: u64,
    hamming: Vec<f32>,
    windowed: Vec<f32>,
    emphasized: Vec<f32>,
    finder: FormantFinder,
    floor_db: f32,
    open: f32,
    history: Vec<[f32; VOWEL_COUNT]>,
    history_len: usize,
    history_at: usize,
    held: [f32; VOWEL_COUNT],
}

impl Lipsync {
    /// Rejects sample rates outside 8..=192 kHz and invalid settings.
    pub fn new(sample_rate: u32, cfg: LipsyncConfig) -> Result<Self, AiError> {
        if !(8_000..=192_000).contains(&sample_rate) {
            return Err(AiError::Lipsync(format!(
                "sample rate {sample_rate} Hz is outside 8000..=192000"
            )));
        }
        cfg.validate()?;
        let decimation = ((sample_rate as f32 / ANALYSIS_RATE) as usize).max(1);
        let rate = sample_rate as f32 / decimation as f32;
        let finder = FormantFinder::new(rate);
        let win = ((cfg.window_ms * rate / 1000.0).round() as usize).max(1);
        let hop = ((cfg.hop_ms * rate / 1000.0).round() as usize).max(1);
        if win <= finder.order() + 1 {
            return Err(AiError::Lipsync("the window is too short".into()));
        }
        let lags = [
            (rate / F0_RANGE[1]) as usize,
            ((rate / F0_RANGE[0]) as usize).min(win - 1),
        ];
        let hop_s = hop as f32 * decimation as f32 / sample_rate as f32;
        Ok(Self {
            decimation,
            acc: 0.0,
            acc_n: 0,
            win,
            hop,
            lags,
            hop_s,
            attack: coefficient(cfg.attack_ms, hop_s),
            release: coefficient(cfg.release_ms, hop_s),
            ring: vec![0.0; win],
            head: 0,
            total: 0,
            hamming: (0..win)
                .map(|i| 0.54 - 0.46 * (TAU * i as f32 / (win - 1).max(1) as f32).cos())
                .collect(),
            windowed: vec![0.0; win],
            emphasized: vec![0.0; win],
            finder,
            floor_db: cfg.silence_db - cfg.floor_margin_db,
            open: 0.0,
            history: vec![[0.0; VOWEL_COUNT]; cfg.vowel_frames],
            history_len: 0,
            history_at: 0,
            held: [0.0; VOWEL_COUNT],
            cfg,
        })
    }

    /// Seconds between frames.
    pub fn hop_seconds(&self) -> f32 {
        self.hop_s
    }

    /// Seconds of audio a frame is computed from; it describes the middle.
    pub fn window_seconds(&self) -> f32 {
        self.hop_s * self.win as f32 / self.hop as f32
    }

    /// Delay from sound to mouth: the wait for the window's center, the lag
    /// of the vowel moving average and the opening's rise time.
    pub fn latency(&self) -> Duration {
        let ms = self.window_seconds() * 500.0
            + (self.cfg.vowel_frames - 1) as f32 * self.hop_s * 500.0
            + self.cfg.attack_ms;
        Duration::from_secs_f32(ms / 1000.0)
    }

    /// Feeds mono samples of any length; `emit` gets one frame per hop. The
    /// frames depend only on the samples, not on how they are split across
    /// calls. Non-finite samples count as silence.
    pub fn push(&mut self, pcm: &[f32], mut emit: impl FnMut(MouthFrame)) {
        for &x in pcm {
            self.acc += if x.is_finite() {
                x.clamp(-1e3, 1e3)
            } else {
                0.0
            };
            self.acc_n += 1;
            if self.acc_n == self.decimation {
                let s = self.acc / self.decimation as f32;
                (self.acc, self.acc_n) = (0.0, 0);
                self.ring[self.head] = s;
                self.head = (self.head + 1) % self.win;
                self.total += 1;
                let (win, hop) = (self.win as u64, self.hop as u64);
                if self.total >= win && (self.total - win).is_multiple_of(hop) {
                    let frame = self.frame();
                    emit(frame);
                }
            }
        }
    }

    fn frame(&mut self) -> MouthFrame {
        let n = self.win;
        // Loudness and the mean over the ring (oldest sample first).
        let (mut sum, mut sumsq) = (0.0f32, 0.0f32);
        for &s in &self.ring {
            sum += s;
            sumsq += s * s;
        }
        let mean = sum / n as f32;
        let db = 10.0 * (sumsq / n as f32 + 1e-12).log10();
        for i in 0..n {
            let s = self.ring[(self.head + i) % n] - mean;
            self.windowed[i] = s * self.hamming[i];
        }

        // Noise floor: follows quiet instantly, rises slowly under sound.
        if db < self.floor_db {
            self.floor_db = db.max(-90.0);
        } else {
            self.floor_db =
                (self.floor_db + self.cfg.floor_rise_db_per_s * self.hop_s).min(MAX_FLOOR_DB);
        }
        let low = (self.floor_db + self.cfg.floor_margin_db).max(self.cfg.silence_db);
        let loud = ((db - low) / (self.cfg.full_db - low)).clamp(0.0, 1.0);

        let voiced = loud > 0.0 && self.periodicity() >= self.cfg.voiced_threshold;
        let mut target = loud;
        let mut weights = None;
        if voiced {
            for i in 0..n {
                let prev = if i > 0 { self.windowed[i - 1] } else { 0.0 };
                self.emphasized[i] = self.windowed[i] - PRE_EMPHASIS * prev;
            }
            match self.finder.find(&self.emphasized) {
                Some(f) => weights = Some(self.classify(f)),
                None => target *= UNSURE_OPEN,
            }
        }
        let rate = if target > self.open {
            self.attack
        } else {
            self.release
        };
        self.open += (target - self.open) * rate;

        if let Some(w) = weights {
            self.history[self.history_at] = w;
            self.history_at = (self.history_at + 1) % self.history.len();
            self.history_len = (self.history_len + 1).min(self.history.len());
            let mut avg = [0.0; VOWEL_COUNT];
            for h in &self.history[..self.history_len] {
                for (a, v) in avg.iter_mut().zip(h) {
                    *a += v / self.history_len as f32;
                }
            }
            self.held = avg;
        }
        MouthFrame {
            open: self.open,
            vowels: self.held,
            voiced,
        }
    }

    /// The highest normalized autocorrelation within the pitch lag range.
    fn periodicity(&self) -> f32 {
        let w = &self.windowed;
        let energy: f32 = w.iter().map(|x| x * x).sum();
        if energy <= 0.0 {
            return 0.0;
        }
        (self.lags[0]..=self.lags[1])
            .map(|lag| w[lag..].iter().zip(w).map(|(a, b)| a * b).sum::<f32>() / energy)
            .fold(0.0, f32::max)
    }

    /// Vowel weights from the distance of (F1, F2) to the five targets.
    fn classify(&self, f: [f32; 2]) -> [f32; VOWEL_COUNT] {
        let f = [
            f[0] * self.cfg.formant_scale[0],
            f[1] * self.cfg.formant_scale[1],
        ];
        let mut d = [0.0f32; VOWEL_COUNT];
        for (d, t) in d.iter_mut().zip(&self.cfg.vowels) {
            let (a, b) = (
                (f[0] - t[0]) / self.cfg.formant_sigma[0],
                (f[1] - t[1]) / self.cfg.formant_sigma[1],
            );
            *d = a * a + b * b;
        }
        let nearest = d.iter().copied().fold(f32::MAX, f32::min);
        let mut w = d.map(|d| (-0.5 * (d - nearest)).exp());
        let total: f32 = w.iter().sum();
        w.iter_mut().for_each(|v| *v /= total);
        w
    }
}
