// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Motion curve sampling by time, independent of any frame rate.

use tsukumoe_core::tmod::playback::{
    SEGMENT_BEZIER, SEGMENT_INVERSE_STEPPED, SEGMENT_STEPPED, SegmentDef,
};

/// The value at time `t` of the curve that starts at `start` (`[time, value]`)
/// and continues with `segments` (times never run backwards; a Bezier's
/// control times lie between its ends: `PlaybackData::check`).
///
/// - before the start, and with no segment, the start value
/// - a segment holds from its start time up to (excluding) its end time; at
///   a shared time the later segment wins
/// - after the last segment, its end value
/// - linear interpolates; Bezier solves the cubic time axis for the value;
///   stepped holds the previous value until the end time; inverse stepped
///   takes the new value at the start time
pub fn sample(start: [f32; 2], segments: &[SegmentDef], t: f32) -> f32 {
    let Some(last) = segments.last() else {
        return start[1];
    };
    // Also catches NaN.
    if t.is_nan() || t < start[0] {
        return start[1];
    }
    // Segments whose end is not after `t` are behind us; zero-length ones
    // fall in that group and never divide.
    let i = segments.partition_point(|s| s.end_time <= t);
    let Some(s) = segments.get(i) else {
        return last.end_value;
    };
    let (t0, v0) = match i.checked_sub(1) {
        Some(k) => (segments[k].end_time, segments[k].end_value),
        None => (start[0], start[1]),
    };
    match s.kind {
        SEGMENT_STEPPED => v0,
        SEGMENT_INVERSE_STEPPED => s.end_value,
        SEGMENT_BEZIER => bezier([t0, v0], s, t),
        _ => v0 + (s.end_value - v0) * ((t - t0) / (s.end_time - t0)),
    }
}

fn bezier(p0: [f32; 2], s: &SegmentDef, t: f32) -> f32 {
    let times = [p0[0], s.c1_time, s.c2_time, s.end_time];
    let values = [p0[1], s.c1_value, s.c2_value, s.end_value];
    let cubic = |c: [f32; 4], u: f32| {
        let v = 1.0 - u;
        v * v * v * c[0] + 3.0 * v * v * u * c[1] + 3.0 * v * u * u * c[2] + u * u * u * c[3]
    };
    // The time axis is monotonic, so bisection finds the parameter.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if cubic(times, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic(values, 0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::tmod::playback::SEGMENT_LINEAR;

    fn seg(kind: u32, end: [f32; 2]) -> SegmentDef {
        SegmentDef {
            kind,
            end_time: end[0],
            end_value: end[1],
            c1_time: 0.0,
            c1_value: 0.0,
            c2_time: 0.0,
            c2_value: 0.0,
        }
    }

    fn bez(c1: [f32; 2], c2: [f32; 2], end: [f32; 2]) -> SegmentDef {
        SegmentDef {
            kind: SEGMENT_BEZIER,
            c1_time: c1[0],
            c1_value: c1[1],
            c2_time: c2[0],
            c2_value: c2[1],
            ..seg(SEGMENT_BEZIER, end)
        }
    }

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "{a} != {b}");
    }

    #[test]
    fn linear_hits_the_ends_and_the_middle() {
        let s = [seg(SEGMENT_LINEAR, [2.0, 4.0])];
        near(sample([0.0, 0.0], &s, 0.0), 0.0);
        near(sample([0.0, 0.0], &s, 1.0), 2.0);
        near(sample([0.0, 0.0], &s, 2.0), 4.0);
        // Outside the curve the end values hold.
        near(sample([0.0, 0.0], &s, -1.0), 0.0);
        near(sample([0.0, 0.0], &s, 9.0), 4.0);
    }

    #[test]
    fn stepped_holds_then_jumps_and_inverse_stepped_jumps_first() {
        let s = [
            seg(SEGMENT_STEPPED, [1.0, 5.0]),
            seg(SEGMENT_INVERSE_STEPPED, [2.0, 7.0]),
        ];
        near(sample([0.0, 1.0], &s, 0.0), 1.0);
        near(sample([0.0, 1.0], &s, 0.999), 1.0);
        // At the shared time 1.0 the later segment starts: it jumps at once.
        near(sample([0.0, 1.0], &s, 1.0), 7.0);
        near(sample([0.0, 1.0], &s, 1.5), 7.0);
        near(sample([0.0, 1.0], &s, 2.0), 7.0);
    }

    #[test]
    fn a_bezier_with_a_symmetric_shape_passes_through_its_middle() {
        // An S curve from (0, 0) to (1, 1): the point at the middle time is the middle value.
        let s = [bez([0.25, 0.0], [0.75, 1.0], [1.0, 1.0])];
        near(sample([0.0, 0.0], &s, 0.0), 0.0);
        near(sample([0.0, 0.0], &s, 0.5), 0.5);
        near(sample([0.0, 0.0], &s, 1.0), 1.0);
        // Antisymmetric about the middle.
        near(
            sample([0.0, 0.0], &s, 0.25) + sample([0.0, 0.0], &s, 0.75),
            1.0,
        );
        // Flat start: the curve stays low early.
        assert!(sample([0.0, 0.0], &s, 0.1) < 0.1);
    }

    #[test]
    fn a_bezier_with_evenly_spaced_control_times_follows_the_value_cubic() {
        // Times at thirds make time itself the Bezier parameter.
        let s = [bez([1.0 / 3.0, 1.0], [2.0 / 3.0, 0.0], [1.0, 0.0])];
        // y(u) = 3(1-u)^2 u + 0 ... at u = 0.5: 3 * 0.25 * 0.5 = 0.375.
        near(sample([0.0, 0.0], &s, 0.5), 0.375);
        near(sample([0.0, 0.0], &s, 0.25), 0.0 + 3.0 * 0.5625 * 0.25);
    }

    #[test]
    fn segment_boundaries_are_continuous_for_linear_and_bezier() {
        let s = [
            seg(SEGMENT_LINEAR, [1.0, 2.0]),
            bez([1.25, 3.0], [1.75, 1.0], [2.0, 2.0]),
        ];
        assert!((sample([0.0, 0.0], &s, 1.0 - 1e-4) - 2.0).abs() < 1e-3);
        near(sample([0.0, 0.0], &s, 1.0), 2.0);
        assert!((sample([0.0, 0.0], &s, 1.0 + 1e-4) - 2.0).abs() < 3e-3);
        near(sample([0.0, 0.0], &s, 2.0), 2.0);
    }

    #[test]
    fn empty_curves_and_zero_length_segments_do_not_break() {
        near(sample([0.5, 3.0], &[], 7.0), 3.0);
        // A zero-length linear segment is an instant jump to its end value.
        let s = [seg(SEGMENT_LINEAR, [0.0, 4.0])];
        near(sample([0.0, 1.0], &s, 0.0), 4.0);
        near(sample([0.0, 1.0], &s, 1.0), 4.0);
        let z = [bez([0.0, 9.0], [0.0, 9.0], [0.0, 2.0])];
        assert!(sample([0.0, 1.0], &z, 0.0).is_finite());
        assert!(sample([0.0, 1.0], &s, f32::NAN).is_finite());
    }
}
