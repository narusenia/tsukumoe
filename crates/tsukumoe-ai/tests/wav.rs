// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The WAV reader: every sample format, channel layout and chunk quirk, and
//! mutated files (never a panic).

use tsukumoe_ai::wav::{self, MAX_WAV_BYTES};

/// A WAV file with `fmt ` (`tag`, `channels`, `bits`) and `data`.
fn file(tag: u16, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
    let rate = 16_000u32;
    let mut fmt = Vec::new();
    fmt.extend(tag.to_le_bytes());
    fmt.extend(channels.to_le_bytes());
    fmt.extend(rate.to_le_bytes());
    fmt.extend((rate * u32::from(channels) * u32::from(bits) / 8).to_le_bytes());
    fmt.extend((channels * bits / 8).to_le_bytes());
    fmt.extend(bits.to_le_bytes());
    let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
    out.extend(b"fmt ");
    out.extend((fmt.len() as u32).to_le_bytes());
    out.extend(fmt);
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    let n = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&n.to_le_bytes());
    out
}

#[test]
fn every_bit_depth_decodes() {
    let w = wav::parse(&file(1, 1, 8, &[128, 255, 0])).unwrap();
    assert_eq!(w.sample_rate, 16_000);
    assert_eq!(w.samples, [0.0, 127.0 / 128.0, -1.0]);
    let w = wav::parse(&file(1, 1, 16, &[0, 0x40, 0, 0x80])).unwrap();
    assert_eq!(w.samples, [0.5, -1.0]);
    let w = wav::parse(&file(1, 1, 24, &[0, 0, 0x40, 0, 0, 0x80])).unwrap();
    assert_eq!(w.samples, [0.5, -1.0]);
    let w = wav::parse(&file(1, 1, 32, &[0, 0, 0, 0x40, 0, 0, 0, 0x80])).unwrap();
    assert_eq!(w.samples, [0.5, -1.0]);
    let mut f = 0.25f32.to_le_bytes().to_vec();
    f.extend(f32::NAN.to_le_bytes());
    let w = wav::parse(&file(3, 1, 32, &f)).unwrap();
    assert_eq!(w.samples, [0.25, 0.0]);
}

#[test]
fn stereo_is_averaged() {
    let w = wav::parse(&file(1, 2, 16, &[0, 0x40, 0, 0xC0, 0, 0x20, 0, 0x20])).unwrap();
    assert_eq!(w.samples, [0.0, 0.25]);
}

#[test]
fn unknown_chunks_odd_padding_and_short_data_are_handled() {
    let base = file(1, 1, 16, &[0, 0x40, 0, 0x20, 7]);
    // Insert an odd-sized (padded) LIST chunk before `fmt `.
    let mut f = base[..12].to_vec();
    f.extend(b"LIST\x03\0\0\0abc\0");
    f.extend(&base[12..]);
    // `data` claims more than the file holds; the trailing odd byte is dropped.
    let at = f.len() - 5 - 4;
    f[at..at + 4].copy_from_slice(&1_000_000u32.to_le_bytes());
    let w = wav::parse(&f).unwrap();
    assert_eq!(w.samples, [0.5, 0.25]);
}

#[test]
fn bad_files_are_errors() {
    let good = file(1, 1, 16, &[0, 0, 0, 0]);
    assert!(wav::parse(b"").is_err());
    assert!(wav::parse(b"RIFF\0\0\0\0WAVE").is_err());
    assert!(wav::parse(b"not a wav file at all").is_err());
    assert!(wav::parse(&file(1, 1, 16, &[])).is_err());
    assert!(wav::parse(&file(1, 3, 16, &[0; 6])).is_err());
    assert!(wav::parse(&file(1, 1, 12, &[0; 6])).is_err());
    assert!(wav::parse(&file(2, 1, 16, &[0; 6])).is_err()); // ADPCM
    assert!(wav::parse(&file(3, 1, 16, &[0; 6])).is_err());
    let mut f = good.clone();
    f[8..12].copy_from_slice(b"AVI ");
    assert!(wav::parse(&f).is_err());
    let mut f = good.clone();
    f[24..28].copy_from_slice(&0u32.to_le_bytes()); // sample rate 0
    assert!(wav::parse(&f).is_err());
    let mut f = good;
    f[16..20].copy_from_slice(&u32::MAX.to_le_bytes()); // huge fmt size
    assert!(wav::parse(&f).is_err());
    assert!(wav::parse(&vec![0; MAX_WAV_BYTES + 1]).is_err());
}

#[test]
fn mutated_files_never_panic() {
    let mut data = Vec::new();
    for i in 0..64i16 {
        data.extend((i * 300).to_le_bytes());
    }
    let seeds = [
        file(1, 1, 16, &data),
        file(1, 2, 24, &[1; 60]),
        file(3, 1, 32, &[0; 16]),
    ];
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for seed in &seeds {
        for i in 0..2000 {
            let mut b = seed.clone();
            match i % 4 {
                0 => b.truncate(next() as usize % (b.len() + 1)),
                1 => {
                    let at = next() as usize % b.len();
                    b[at] = next() as u8;
                }
                2 => {
                    let at = next() as usize % (b.len() - 3);
                    b[at..at + 4].copy_from_slice(&[0xFF; 4]);
                }
                _ => {
                    let at = next() as usize % (b.len() - 1);
                    b[at] ^= 1 << (next() % 8);
                    b.drain(..next() as usize % 4);
                }
            }
            let _ = wav::parse(&b);
        }
    }
}
