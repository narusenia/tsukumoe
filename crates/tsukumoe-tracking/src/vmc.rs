// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! VMC protocol decoding: OSC messages to [`TrackingFrame`]. No I/O.
//!
//! Handled addresses: `/VMC/Ext/Blend/Val`, `/VMC/Ext/Blend/Apply`,
//! `/VMC/Ext/Bone/Pos` (`Head` only: rotation) and `/VMC/Ext/Root/Pos`
//! (position becomes `head_position`). Everything else is skipped.

use crate::frame::{ARKIT_COUNT, TrackingFrame, arkit_index};
use crate::osc::{Arg, Message, parse_packet};

/// Default VMC receive port.
pub const DEFAULT_PORT: u16 = 39539;

/// VRM standard blendshape name to nearby ARKit items. The mapping is
/// deliberately coarse; M4-02 owns the real parameter mapping.
///
/// | VRM        | ARKit                              |
/// |------------|------------------------------------|
/// | `A`        | `jawOpen`                          |
/// | `I`        | `mouthStretchLeft` / `Right`       |
/// | `U`        | `mouthPucker`                      |
/// | `E`        | `mouthSmileLeft` / `Right`         |
/// | `O`        | `mouthFunnel`                      |
/// | `Blink`    | `eyeBlinkLeft` / `Right`           |
/// | `BlinkL`   | `eyeBlinkLeft`                     |
/// | `BlinkR`   | `eyeBlinkRight`                    |
///
/// Any other name (`Joy`, `Neutral`, ...) is dropped and counted as unmapped.
const VRM_MAP: &[(&str, &[&str])] = &[
    ("A", &["jawOpen"]),
    ("I", &["mouthStretchLeft", "mouthStretchRight"]),
    ("U", &["mouthPucker"]),
    ("E", &["mouthSmileLeft", "mouthSmileRight"]),
    ("O", &["mouthFunnel"]),
    ("Blink", &["eyeBlinkLeft", "eyeBlinkRight"]),
    ("BlinkL", &["eyeBlinkLeft"]),
    ("BlinkR", &["eyeBlinkRight"]),
];

/// Quaternions whose norm differs from 1 by more than this are rejected.
const QUAT_NORM_TOLERANCE: f32 = 0.05;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DecodeStats {
    pub packets: u64,
    /// Packets or elements dropped as malformed (OSC level).
    pub malformed: u64,
    /// Blendshape names with no ARKit counterpart.
    pub unmapped: u64,
    /// Messages dropped for NaN / inf, wrong argument shape or bad quaternion.
    pub invalid: u64,
}

/// Stateful decoder: blendshape values are staged until `Blend/Apply`.
#[derive(Debug)]
pub struct VmcDecoder {
    frame: TrackingFrame,
    pending: [Option<f32>; ARKIT_COUNT],
    pub stats: DecodeStats,
}

fn float(a: &Arg) -> Option<f32> {
    match a {
        Arg::Float(v) if v.is_finite() => Some(*v),
        _ => None,
    }
}

/// Floats at `args[1..1 + N]`; `None` if missing, mistyped, or non-finite.
fn floats<const N: usize>(args: &[Arg]) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for (o, a) in out.iter_mut().zip(args.get(1..)?) {
        *o = float(a)?;
    }
    (args.len() > N).then_some(out)
}

/// Calls `f` for every ARKit index `name` maps to; false if none.
fn for_each_target(name: &str, mut f: impl FnMut(usize)) -> bool {
    if let Some(i) = arkit_index(name) {
        f(i);
        return true;
    }
    let Some((_, to)) = VRM_MAP.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) else {
        return false;
    };
    to.iter().filter_map(|t| arkit_index(t)).for_each(f);
    true
}

impl Default for VmcDecoder {
    fn default() -> Self {
        Self {
            frame: TrackingFrame::default(),
            pending: [None; ARKIT_COUNT],
            stats: DecodeStats::default(),
        }
    }
}

impl VmcDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The most recent applied state.
    pub fn frame(&self) -> &TrackingFrame {
        &self.frame
    }

    /// Decodes one datagram stamped `time`. Returns true if the frame changed.
    pub fn decode_packet(&mut self, bytes: &[u8], time: f64) -> bool {
        self.stats.packets += 1;
        let packet = match parse_packet(bytes) {
            Ok(p) => p,
            Err(_) => {
                self.stats.malformed += 1;
                return false;
            }
        };
        self.stats.malformed += packet.dropped as u64;
        let mut changed = false;
        for m in &packet.messages {
            changed |= self.handle(m, time);
        }
        changed
    }

    fn handle(&mut self, m: &Message, time: f64) -> bool {
        match m.address.as_str() {
            "/VMC/Ext/Blend/Val" => {
                self.blend_val(&m.args);
                false
            }
            "/VMC/Ext/Blend/Apply" => {
                for (dst, p) in self.frame.blend.iter_mut().zip(&mut self.pending) {
                    if let Some(v) = p.take() {
                        *dst = v.clamp(0.0, 1.0);
                    }
                }
                self.frame.time = time;
                true
            }
            "/VMC/Ext/Bone/Pos" => self.bone_pos(&m.args, time),
            "/VMC/Ext/Root/Pos" => self.root_pos(&m.args, time),
            _ => false,
        }
    }

    fn blend_val(&mut self, args: &[Arg]) {
        let (Some(Arg::Str(name)), Some(v)) = (args.first(), args.get(1).and_then(float)) else {
            self.stats.invalid += 1;
            return;
        };
        let pending = &mut self.pending;
        if !for_each_target(name, |i| pending[i] = Some(v)) {
            self.stats.unmapped += 1;
        }
    }

    fn bone_pos(&mut self, args: &[Arg], time: f64) -> bool {
        let Some(Arg::Str(name)) = args.first() else {
            self.stats.invalid += 1;
            return false;
        };
        if name != "Head" {
            return false;
        }
        let Some([_, _, _, x, y, z, w]) = floats::<7>(args) else {
            self.stats.invalid += 1;
            return false;
        };
        let norm = (x * x + y * y + z * z + w * w).sqrt();
        if (norm - 1.0).abs() > QUAT_NORM_TOLERANCE {
            self.stats.invalid += 1;
            return false;
        }
        self.frame.head_rotation = [x / norm, y / norm, z / norm, w / norm];
        self.frame.time = time;
        true
    }

    fn root_pos(&mut self, args: &[Arg], time: f64) -> bool {
        let Some([x, y, z, ..]) = floats::<7>(args) else {
            self.stats.invalid += 1;
            return false;
        };
        self.frame.head_position = Some([x, y, z]);
        self.frame.time = time;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::enc::{bundle, message};

    fn val(name: &str, v: f32) -> Vec<u8> {
        message(
            "/VMC/Ext/Blend/Val",
            &[Arg::Str(name.into()), Arg::Float(v)],
        )
    }
    fn apply() -> Vec<u8> {
        message("/VMC/Ext/Blend/Apply", &[])
    }
    fn bone(name: &str, p: [f32; 3], q: [f32; 4]) -> Vec<u8> {
        let mut a = vec![Arg::Str(name.into())];
        a.extend(p.iter().chain(&q).map(|v| Arg::Float(*v)));
        message("/VMC/Ext/Bone/Pos", &a)
    }
    fn idx(n: &str) -> usize {
        arkit_index(n).unwrap()
    }

    #[test]
    fn realistic_stream() {
        let mut d = VmcDecoder::new();
        let pkt = bundle(&[
            val("jawOpen", 0.25),
            val("JawOpen_ignored", 0.9),
            val("eyeBlinkLeft", 0.75),
            val("A", 0.5),
            val("Joy", 1.0),
            bone("Hips", [0.0; 3], [0.0, 0.0, 0.0, 1.0]),
            bone("Head", [0.0; 3], [0.0, 0.0, 0.0, 1.0]),
            message("/VMC/Ext/OK", &[Arg::Int(1)]),
            message(
                "/VMC/Ext/Root/Pos",
                &[
                    Arg::Str("root".into()),
                    Arg::Float(1.0),
                    Arg::Float(2.0),
                    Arg::Float(3.0),
                    Arg::Float(0.0),
                    Arg::Float(0.0),
                    Arg::Float(0.0),
                    Arg::Float(1.0),
                ],
            ),
        ]);
        assert!(d.decode_packet(&pkt, 1.0));
        // Not applied yet: blend stays zero, pose and position are live.
        assert_eq!(d.frame().blend, [0.0; ARKIT_COUNT]);
        assert_eq!(d.frame().head_position, Some([1.0, 2.0, 3.0]));
        assert_eq!(d.stats.unmapped, 2); // JawOpen_ignored, Joy
        assert!(d.decode_packet(&apply(), 2.0));
        let f = d.frame();
        // `A` came after `jawOpen`, so it wins.
        assert_eq!(f.blend[idx("jawOpen")], 0.5);
        assert_eq!(f.blend[idx("eyeBlinkLeft")], 0.75);
        assert_eq!(f.blend.iter().filter(|v| **v != 0.0).count(), 2);
        assert_eq!(f.time, 2.0);
    }

    #[test]
    fn vrm_names_map_and_values_clamp() {
        let mut d = VmcDecoder::new();
        for (n, v) in [("I", 2.0), ("Blink", 0.5), ("BlinkR", 0.25), ("U", -1.0)] {
            d.decode_packet(&val(n, v), 0.0);
        }
        d.decode_packet(&apply(), 0.0);
        let b = d.frame().blend;
        assert_eq!(b[idx("mouthStretchLeft")], 1.0);
        assert_eq!(b[idx("mouthStretchRight")], 1.0);
        assert_eq!(b[idx("eyeBlinkLeft")], 0.5);
        assert_eq!(b[idx("eyeBlinkRight")], 0.25);
        assert_eq!(b[idx("mouthPucker")], 0.0);
    }

    #[test]
    fn invalid_messages_are_dropped_individually() {
        let mut d = VmcDecoder::new();
        let pkt = bundle(&[
            val("jawOpen", f32::NAN),
            val("eyeBlinkLeft", f32::INFINITY),
            val("browInnerUp", 0.5),
            bone("Head", [0.0; 3], [0.0, 0.0, 0.0, 3.0]), // not normalized
            bone("Head", [0.0; 3], [0.0, f32::NAN, 0.0, 1.0]),
            message("/VMC/Ext/Blend/Val", &[Arg::Int(1)]),
            apply(),
        ]);
        assert!(d.decode_packet(&pkt, 1.0));
        assert_eq!(d.stats.invalid, 5);
        assert_eq!(d.frame().blend[idx("browInnerUp")], 0.5);
        assert_eq!(d.frame().blend[idx("jawOpen")], 0.0);
        assert_eq!(d.frame().head_rotation, [0.0, 0.0, 0.0, 1.0]);
        // Garbage does not stop later decoding.
        assert!(!d.decode_packet(b"\xff\xff", 2.0));
        assert_eq!(d.stats.malformed, 1);
        assert!(d.decode_packet(&bone("Head", [0.0; 3], [0.0, 1.0, 0.0, 0.0]), 3.0));
        assert_eq!(d.frame().head_rotation, [0.0, 1.0, 0.0, 0.0]);
    }
}
