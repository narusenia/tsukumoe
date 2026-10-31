// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A minimal WAV reader: RIFF/WAVE, PCM 8/16/24/32-bit integer and 32-bit
//! float, mono or stereo (averaged to mono). Chunks other than `fmt ` and
//! `data` are skipped. The input is untrusted: every length is checked
//! against the bytes at hand and nothing here panics.

use crate::error::AiError;

/// Larger inputs are rejected before any decoding.
pub const MAX_WAV_BYTES: usize = 128 * 1024 * 1024;

/// Decoded mono audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    pub sample_rate: u32,
    /// Mono samples in -1..1 (non-finite float samples read as 0).
    pub samples: Vec<f32>,
}

impl Wav {
    pub fn duration(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }
}

fn bad<T>(msg: &str) -> Result<T, AiError> {
    Err(AiError::Wav(msg.to_owned()))
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

struct Format {
    float: bool,
    channels: usize,
    sample_rate: u32,
    bits: usize,
}

fn parse_fmt(c: &[u8]) -> Result<Format, AiError> {
    let (Some(mut tag), Some(channels), Some(sample_rate), Some(bits)) =
        (u16_at(c, 0), u16_at(c, 2), u32_at(c, 4), u16_at(c, 14))
    else {
        return bad("truncated fmt chunk");
    };
    if tag == 0xFFFE {
        // WAVE_FORMAT_EXTENSIBLE: the first two bytes of the sub-format GUID.
        match u16_at(c, 24) {
            Some(t) if c.len() >= 40 => tag = t,
            _ => return bad("truncated extensible fmt chunk"),
        }
    }
    let float = match (tag, bits) {
        (1, 8 | 16 | 24 | 32) => false,
        (3, 32) => true,
        _ => return bad("unsupported sample format"),
    };
    if !(1..=2).contains(&channels) {
        return bad("only mono and stereo are supported");
    }
    if sample_rate == 0 {
        return bad("sample rate is 0");
    }
    Ok(Format {
        float,
        channels: channels as usize,
        sample_rate,
        bits: bits as usize,
    })
}

fn sample(f: &Format, b: &[u8]) -> f32 {
    let v = match (f.float, f.bits) {
        (true, _) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        (_, 8) => (f32::from(b[0]) - 128.0) / 128.0,
        (_, 16) => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
        (_, 24) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
        _ => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
    };
    if v.is_finite() { v } else { 0.0 }
}

/// Decodes a WAV file. A `data` chunk that claims more than the file holds is
/// read up to the end of the file (whole frames only).
pub fn parse(bytes: &[u8]) -> Result<Wav, AiError> {
    if bytes.len() > MAX_WAV_BYTES {
        return bad("file is too large");
    }
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return bad("not a RIFF/WAVE file");
    }
    let mut format = None;
    let mut data = None;
    let mut at = 12;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), u32_at(bytes, at + 4)) {
        let start = at + 8;
        let end = start.saturating_add(size as usize).min(bytes.len());
        match id {
            b"fmt " if format.is_none() => format = Some(parse_fmt(&bytes[start..end])?),
            b"data" if data.is_none() => data = Some(&bytes[start..end]),
            _ => {}
        }
        // Chunks are padded to an even size.
        at = start
            .saturating_add(size as usize)
            .saturating_add(size as usize & 1);
    }
    let Some(f) = format else {
        return bad("missing fmt chunk");
    };
    let Some(data) = data else {
        return bad("missing data chunk");
    };
    let width = f.bits / 8;
    let frame = width * f.channels;
    let samples: Vec<f32> = data
        .chunks_exact(frame)
        .map(|fr| {
            let sum: f32 = fr.chunks_exact(width).map(|s| sample(&f, s)).sum();
            sum / f.channels as f32
        })
        .collect();
    if samples.is_empty() {
        return bad("no samples");
    }
    Ok(Wav {
        sample_rate: f.sample_rate,
        samples,
    })
}
