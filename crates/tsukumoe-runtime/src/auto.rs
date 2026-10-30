// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Automatic motion: blinking and breathing. Which parameters they write
//! comes from the configuration, by name; there are no default names.

use crate::Error;

/// Which automatic motions run. The default runs none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AutoConfig {
    pub blink: Option<BlinkConfig>,
    pub breath: Option<BreathConfig>,
}

/// Blinking. The eyes close, stay closed and open again; the wait until the
/// next blink is `interval` seconds times a random factor in 0.5 to 1.5.
/// The eye parameters are multiplied by the openness (1 open, 0 closed).
#[derive(Debug, Clone, PartialEq)]
pub struct BlinkConfig {
    pub left_eye: String,
    pub right_eye: String,
    /// Seconds between blinks on average. Must be at least 0.001.
    pub interval: f32,
    pub close_time: f32,
    pub closed_time: f32,
    pub open_time: f32,
    /// The random sequence is fixed by this seed.
    pub seed: u64,
}

impl BlinkConfig {
    /// Blinks about every 4 seconds, closing in 0.1 s and opening in 0.2 s.
    pub fn new(left_eye: impl Into<String>, right_eye: impl Into<String>) -> Self {
        Self {
            left_eye: left_eye.into(),
            right_eye: right_eye.into(),
            interval: 4.0,
            close_time: 0.1,
            closed_time: 0.05,
            open_time: 0.2,
            seed: 1,
        }
    }
}

/// Breathing: `offset + amplitude * sin(2 pi t / cycle)` is added to the parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct BreathConfig {
    pub parameter: String,
    /// Seconds per breath; positive.
    pub cycle: f32,
    pub amplitude: f32,
    pub offset: f32,
}

/// xorshift64*: small, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// In 0 to 1, 1 excluded.
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Blink {
    left: usize,
    right: usize,
    interval: f64,
    close: f64,
    closed: f64,
    open: f64,
    rng: Rng,
    /// Time of the next (or running) blink's start.
    start: f64,
}

impl Blink {
    fn gap(&mut self) -> f64 {
        self.interval * (0.5 + self.rng.unit())
    }

    fn openness(&mut self, time: f64) -> f32 {
        let total = self.close + self.closed + self.open;
        while time >= self.start + total {
            self.start += total + self.gap();
        }
        let u = time - self.start;
        let open = if u < 0.0 {
            1.0
        } else if u < self.close {
            1.0 - u / self.close
        } else if u < self.close + self.closed {
            0.0
        } else {
            (u - self.close - self.closed) / self.open
        };
        open as f32
    }
}

struct Breath {
    parameter: usize,
    cycle: f64,
    amplitude: f64,
    offset: f64,
}

#[derive(Default)]
pub struct Auto {
    blink: Option<Blink>,
    breath: Option<Breath>,
    /// Seconds since the configuration was set.
    time: f64,
}

fn time_ok(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}

impl Auto {
    /// Replaces the configuration; time starts again from 0. `index` finds a
    /// parameter by name.
    pub fn configure(
        config: &AutoConfig,
        index: impl Fn(&str) -> Option<usize>,
    ) -> Result<Self, Error> {
        let find = |name: &str| index(name).ok_or_else(|| Error::UnknownParameter(name.into()));
        let mut auto = Self::default();
        if let Some(b) = &config.blink {
            let ok = b.interval.is_finite()
                && b.interval >= 0.001
                && time_ok(b.close_time)
                && time_ok(b.closed_time)
                && time_ok(b.open_time);
            if !ok {
                return Err(Error::BadAutoValue("blink times"));
            }
            let mut blink = Blink {
                left: find(&b.left_eye)?,
                right: find(&b.right_eye)?,
                interval: f64::from(b.interval),
                close: f64::from(b.close_time),
                closed: f64::from(b.closed_time),
                open: f64::from(b.open_time),
                rng: Rng::new(b.seed),
                start: 0.0,
            };
            blink.start = blink.gap();
            auto.blink = Some(blink);
        }
        if let Some(b) = &config.breath {
            let ok = b.cycle.is_finite()
                && b.cycle > 0.0
                && b.amplitude.is_finite()
                && b.offset.is_finite();
            if !ok {
                return Err(Error::BadAutoValue("breath cycle, amplitude or offset"));
            }
            auto.breath = Some(Breath {
                parameter: find(&b.parameter)?,
                cycle: f64::from(b.cycle),
                amplitude: f64::from(b.amplitude),
                offset: f64::from(b.offset),
            });
        }
        Ok(auto)
    }

    /// Moves time by `dt` seconds and writes into `work`. Writes nothing
    /// when nothing is configured.
    pub fn update(&mut self, dt: f64, work: &mut [f32]) {
        if self.blink.is_none() && self.breath.is_none() {
            return;
        }
        self.time += dt;
        if let Some(b) = &mut self.blink {
            let o = b.openness(self.time);
            work[b.left] *= o;
            if b.right != b.left {
                work[b.right] *= o;
            }
        }
        if let Some(b) = &self.breath {
            let wave = (std::f64::consts::TAU * self.time / b.cycle).sin();
            work[b.parameter] += (b.offset + b.amplitude * wave) as f32;
        }
    }
}
