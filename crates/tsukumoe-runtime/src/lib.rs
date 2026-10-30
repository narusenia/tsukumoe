// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Playback of a loaded `.tmod`: load, set parameters, play motions and
//! expressions, run automatic motion and physics, and read the draw list.
//!
//! This crate uses only the evaluation side of `tsukumoe-core` (`Frame`,
//! `CompiledModel`, `tmod`, `draw`), never the editing model or the project
//! format; `scripts/check-deps.sh` enforces it.
//!
//! One [`Runtime::update`] runs, in order:
//!
//! 1. every parameter and part opacity returns to its default
//! 2. motions (priorities low to high, layers in order)
//! 3. the pose, on the part opacities the motions wrote
//! 4. expressions, in the order they were started
//! 5. automatic motion (blink, breath)
//! 6. the values set with [`Runtime::set_parameter`], which have the last word
//! 7. `Frame::advance`: drivers, physics and evaluation

pub mod auto;
pub mod curve;
pub mod expression;
pub mod motion;
pub mod pose;

use std::fmt;

use tsukumoe_core::compiled::CompiledModel;
use tsukumoe_core::draw::DrawList;
use tsukumoe_core::eval::Frame;
use tsukumoe_core::layout::LayoutError;
use tsukumoe_core::tmod::{self, AlignedBuf, PlaybackData, Tmod};

pub use auto::{AutoConfig, BlinkConfig, BreathConfig};
pub use motion::{MAX_LAYERS, Priority};

/// The longest step `update` takes at once, in seconds. A longer `dt` is cut
/// to this, so a stall (a hidden tab) does not replay minutes of animation.
pub const MAX_DT: f32 = 10.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The `.tmod` could not be loaded.
    Load(tmod::Error),
    /// The model or playback data is inconsistent.
    Layout(LayoutError),
    UnknownMotion(String),
    UnknownExpression(String),
    UnknownParameter(String),
    /// A layer at or past [`MAX_LAYERS`].
    BadLayer(usize),
    BadAutoValue(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => write!(f, "{e}"),
            Self::Layout(e) => write!(f, "inconsistent data: {e}"),
            Self::UnknownMotion(n) => write!(f, "no motion named '{n}'"),
            Self::UnknownExpression(n) => write!(f, "no expression named '{n}'"),
            Self::UnknownParameter(n) => write!(f, "no parameter named '{n}'"),
            Self::BadLayer(l) => write!(f, "layer {l} is past the last ({})", MAX_LAYERS - 1),
            Self::BadAutoValue(what) => write!(f, "bad value for {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<tmod::Error> for Error {
    fn from(e: tmod::Error) -> Self {
        Self::Load(e)
    }
}

impl From<LayoutError> for Error {
    fn from(e: LayoutError) -> Self {
        Self::Layout(e)
    }
}

pub struct Runtime<'a> {
    model: CompiledModel<'a>,
    playback: PlaybackData<'a>,
    frame: Frame,
    motions: motion::MotionPlayer,
    expressions: expression::ExpressionPlayer,
    pose: pose::PosePlayer,
    auto: auto::Auto,
    user: Vec<Option<f32>>,
    /// Parameter values of the frame being built.
    work: Vec<f32>,
    /// Part opacities of the frame being built.
    parts: Vec<f32>,
    /// Parts a motion has written this frame.
    driven: Vec<bool>,
}

impl<'a> Runtime<'a> {
    /// Loads a `.tmod` in place: nothing of the model is copied. `bytes` must
    /// be the whole file and start on a 16-byte boundary (an [`AlignedBuf`]
    /// does); otherwise use [`Runtime::from_bytes`].
    pub fn load(bytes: &'a [u8]) -> Result<Self, Error> {
        let Tmod {
            model, playback, ..
        } = Tmod::parse(bytes)?;
        Self::new(model, playback)
    }

    /// Builds a runtime from a model and its playback data, which are checked.
    pub fn new(model: CompiledModel<'a>, playback: PlaybackData<'a>) -> Result<Self, Error> {
        model.check()?;
        playback.check(model.parameters.len(), model.part_opacity.len())?;
        Ok(Self {
            frame: Frame::new(&model),
            motions: motion::MotionPlayer::default(),
            expressions: expression::ExpressionPlayer::default(),
            pose: pose::PosePlayer::new(&playback),
            auto: auto::Auto::default(),
            user: vec![None; model.parameters.len()],
            work: model.parameters.iter().map(|p| p.default).collect(),
            parts: model.part_opacity.to_vec(),
            driven: vec![false; model.part_opacity.len()],
            model,
            playback,
        })
    }
}

impl Runtime<'static> {
    /// Loads a `.tmod` from bytes at any alignment, copying what it needs.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let buf = AlignedBuf::from_bytes(bytes);
        let Tmod {
            model, playback, ..
        } = Tmod::parse(&buf)?;
        Self::new(model.into_owned(), playback.into_owned())
    }
}

impl Runtime<'_> {
    pub fn model(&self) -> &CompiledModel<'_> {
        &self.model
    }

    pub fn playback(&self) -> &PlaybackData<'_> {
        &self.playback
    }

    pub fn parameter_index(&self, name: &str) -> Option<usize> {
        self.model.parameter_index(name)
    }

    /// Holds parameter `index` at `value` from the next update on, above
    /// motions, expressions and automatic motion, until
    /// [`Runtime::clear_parameter`]. `false` when there is no such parameter.
    pub fn set_parameter(&mut self, index: usize, value: f32) -> bool {
        match self.user.get_mut(index) {
            Some(slot) => {
                *slot = Some(value);
                true
            }
            None => false,
        }
    }

    /// Gives parameter `index` back to motions, expressions and automatic motion.
    pub fn clear_parameter(&mut self, index: usize) -> bool {
        match self.user.get_mut(index) {
            Some(slot) => {
                *slot = None;
                true
            }
            None => false,
        }
    }

    /// The parameter values of the last update as they went into the
    /// evaluation (before drivers and physics, and before clamping).
    pub fn parameter_values(&self) -> &[f32] {
        &self.work
    }

    /// Plays motion `name` on `layer`. `Ok(false)` when a motion of a higher
    /// priority is playing, which keeps playing.
    pub fn play_motion(
        &mut self,
        name: &str,
        layer: usize,
        priority: Priority,
    ) -> Result<bool, Error> {
        if layer >= MAX_LAYERS {
            return Err(Error::BadLayer(layer));
        }
        let motion = self
            .playback
            .motion_index(name)
            .ok_or_else(|| Error::UnknownMotion(name.into()))?;
        Ok(self.motions.play(&self.playback, motion, layer, priority))
    }

    /// Fades out the motion of `layer`.
    pub fn stop(&mut self, layer: usize) {
        self.motions.stop(layer);
    }

    /// Whether `layer` has a motion that is still going.
    pub fn is_playing(&self, layer: usize) -> bool {
        self.motions.is_playing(&self.playback, layer)
    }

    /// Fades expression `name` in on top of the expressions already active.
    pub fn play_expression(&mut self, name: &str) -> Result<(), Error> {
        let i = self
            .playback
            .expression_index(name)
            .ok_or_else(|| Error::UnknownExpression(name.into()))?;
        self.expressions.play(i);
        Ok(())
    }

    /// Fades expression `name` out.
    pub fn stop_expression(&mut self, name: &str) -> Result<(), Error> {
        let i = self
            .playback
            .expression_index(name)
            .ok_or_else(|| Error::UnknownExpression(name.into()))?;
        self.expressions.stop(i);
        Ok(())
    }

    /// Replaces the automatic motion; its clock starts again. An
    /// `AutoConfig::default()` turns it off. On an error nothing changes.
    pub fn set_auto(&mut self, config: &AutoConfig) -> Result<(), Error> {
        self.auto = auto::Auto::configure(config, |n| self.model.parameter_index(n))?;
        Ok(())
    }

    /// Advances everything by `dt` seconds (negative and non-finite count as
    /// 0, more than [`MAX_DT`] is cut) and evaluates the frame.
    pub fn update(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            f64::from(dt.clamp(0.0, MAX_DT))
        } else {
            0.0
        };
        for (w, p) in self.work.iter_mut().zip(self.model.parameters.iter()) {
            *w = p.default;
        }
        self.parts.copy_from_slice(&self.model.part_opacity);
        self.driven.fill(false);

        self.motions.update(
            &self.playback,
            dt,
            &mut self.work,
            &mut self.parts,
            &mut self.driven,
        );
        self.pose
            .update(&self.playback, dt, &mut self.parts, &self.driven);
        self.expressions.update(&self.playback, dt, &mut self.work);
        self.auto.update(dt, &mut self.work);
        for (w, u) in self.work.iter_mut().zip(&self.user) {
            if let Some(v) = u {
                *w = *v;
            }
        }

        self.frame.parameters_mut().copy_from_slice(&self.work);
        for (i, &v) in self.parts.iter().enumerate() {
            self.frame.set_part_opacity(i, v.clamp(0.0, 1.0));
        }
        self.frame.advance(&self.model, dt as f32);
    }

    /// The draw list of the last update.
    pub fn draw_list(&self) -> DrawList<'_> {
        self.frame.draw_list(&self.model)
    }
}
