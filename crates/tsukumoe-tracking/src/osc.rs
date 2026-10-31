// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Minimal OSC 1.0 parser: `int32` / `float32` / `string` arguments and
//! bundles. Input is untrusted: every size is bounded and nothing panics.

/// Largest accepted packet (the UDP maximum).
pub const MAX_PACKET_LEN: usize = 65_536;
/// Most arguments accepted in one message.
pub const MAX_ARGS: usize = 64;
/// Most messages collected from one packet.
pub const MAX_MESSAGES: usize = 1024;
const MAX_BUNDLE_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i32),
    Float(f32),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub address: String,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscError {
    TooLong,
    Truncated,
    BadAddress,
    BadTypeTag,
    BadString,
    BadBundle,
    TooMany,
}

/// Messages parsed from one packet plus the number of elements discarded.
#[derive(Debug, Default, PartialEq)]
pub struct Packet {
    pub messages: Vec<Message>,
    pub dropped: usize,
}

/// Parses a datagram. A malformed bare message is an `Err`; inside a bundle a
/// malformed element is counted in `dropped` and the rest is still parsed.
pub fn parse_packet(data: &[u8]) -> Result<Packet, OscError> {
    if data.len() > MAX_PACKET_LEN {
        return Err(OscError::TooLong);
    }
    let mut out = Packet::default();
    parse_element(data, 0, &mut out)?;
    Ok(out)
}

fn parse_element(data: &[u8], depth: usize, out: &mut Packet) -> Result<(), OscError> {
    if !data.starts_with(b"#bundle\0") {
        if out.messages.len() >= MAX_MESSAGES {
            return Err(OscError::TooMany);
        }
        out.messages.push(parse_message(data)?);
        return Ok(());
    }
    if depth >= MAX_BUNDLE_DEPTH {
        return Err(OscError::BadBundle);
    }
    // 8 bytes magic + 8 bytes time tag.
    let mut rest = data.get(16..).ok_or(OscError::Truncated)?;
    while !rest.is_empty() {
        let Some(([a, b, c, d], tail)) = rest.split_first_chunk::<4>() else {
            out.dropped += 1;
            break;
        };
        let size = u32::from_be_bytes([*a, *b, *c, *d]) as usize;
        if !size.is_multiple_of(4) || size > tail.len() {
            out.dropped += 1;
            break;
        }
        let (body, next) = tail.split_at(size);
        if parse_element(body, depth + 1, out).is_err() {
            out.dropped += 1;
        }
        rest = next;
    }
    Ok(())
}

/// Reads a NUL-terminated, 4-byte padded string at `off`; returns it and the
/// offset after the padding.
fn read_str(data: &[u8], off: usize) -> Result<(&str, usize), OscError> {
    let rest = data.get(off..).ok_or(OscError::Truncated)?;
    let nul = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(OscError::Truncated)?;
    let s = std::str::from_utf8(&rest[..nul]).map_err(|_| OscError::BadString)?;
    let next = off + ((nul + 4) & !3);
    if next > data.len() {
        return Err(OscError::Truncated);
    }
    Ok((s, next))
}

fn read_word(data: &[u8], off: usize) -> Result<[u8; 4], OscError> {
    match data.get(off..).and_then(|r| r.first_chunk::<4>()) {
        Some(w) => Ok(*w),
        None => Err(OscError::Truncated),
    }
}

fn parse_message(data: &[u8]) -> Result<Message, OscError> {
    let (address, mut off) = read_str(data, 0)?;
    if !address.starts_with('/') {
        return Err(OscError::BadAddress);
    }
    let (tags, next) = read_str(data, off)?;
    off = next;
    let tags = tags.strip_prefix(',').ok_or(OscError::BadTypeTag)?;
    if tags.len() > MAX_ARGS {
        return Err(OscError::TooMany);
    }
    let mut args = Vec::with_capacity(tags.len());
    for t in tags.bytes() {
        match t {
            b'i' => {
                args.push(Arg::Int(i32::from_be_bytes(read_word(data, off)?)));
                off += 4;
            }
            b'f' => {
                args.push(Arg::Float(f32::from_be_bytes(read_word(data, off)?)));
                off += 4;
            }
            b's' => {
                let (s, next) = read_str(data, off)?;
                args.push(Arg::Str(s.to_owned()));
                off = next;
            }
            _ => return Err(OscError::BadTypeTag),
        }
    }
    Ok(Message {
        address: address.to_owned(),
        args,
    })
}

/// Test-only encoder (the crate never sends OSC).
#[cfg(test)]
pub(crate) mod enc {
    use super::Arg;

    fn pad_str(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(s.as_bytes());
        out.push(0);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }

    pub fn message(addr: &str, args: &[Arg]) -> Vec<u8> {
        let mut out = Vec::new();
        pad_str(&mut out, addr);
        let tags: String = std::iter::once(',')
            .chain(args.iter().map(|a| match a {
                Arg::Int(_) => 'i',
                Arg::Float(_) => 'f',
                Arg::Str(_) => 's',
            }))
            .collect();
        pad_str(&mut out, &tags);
        for a in args {
            match a {
                Arg::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
                Arg::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
                Arg::Str(s) => pad_str(&mut out, s),
            }
        }
        out
    }

    pub fn bundle(elements: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"#bundle\0".to_vec();
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        for e in elements {
            out.extend_from_slice(&(e.len() as u32).to_be_bytes());
            out.extend_from_slice(e);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::enc::{bundle, message};
    use super::*;

    fn s(v: &str) -> Arg {
        Arg::Str(v.to_owned())
    }

    #[test]
    fn parses_hand_written_message() {
        // "/a" + pad, ",ifs" + pad, 1, 1.0, "hi" + pad.
        let mut b = b"/a\0\0,ifs\0\0\0\0".to_vec();
        b.extend_from_slice(&[0, 0, 0, 1, 0x3f, 0x80, 0, 0]);
        b.extend_from_slice(b"hi\0\0");
        let p = parse_packet(&b).unwrap();
        assert_eq!(
            p.messages,
            vec![Message {
                address: "/a".into(),
                args: vec![Arg::Int(1), Arg::Float(1.0), s("hi")]
            }]
        );
    }

    #[test]
    fn string_padding_boundaries() {
        // Lengths 0..=8 cross every padding case (a NUL is always added).
        for n in 0..=8 {
            let text = "x".repeat(n);
            let b = message("/p", &[s(&text), Arg::Int(7)]);
            assert_eq!(b.len() % 4, 0);
            let p = parse_packet(&b).unwrap();
            assert_eq!(p.messages[0].args, vec![s(&text), Arg::Int(7)]);
        }
    }

    #[test]
    fn bundle_with_nested_bundle_and_bad_element() {
        let good = message("/g", &[Arg::Float(0.5)]);
        let bad = message("/b", &[Arg::Float(0.5)]);
        let mut bad = bad;
        // Corrupt the type tag into an unsupported one ('b').
        let i = bad.iter().position(|&c| c == b'f').unwrap();
        bad[i] = b'b';
        let inner = bundle(std::slice::from_ref(&good));
        let b = bundle(&[good.clone(), bad, inner]);
        let p = parse_packet(&b).unwrap();
        assert_eq!(p.messages.len(), 2);
        assert_eq!(p.dropped, 1);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse_packet(&[]), Err(OscError::Truncated));
        assert_eq!(
            parse_packet(b"nope\0\0\0\0,\0\0\0"),
            Err(OscError::BadAddress)
        );
        assert_eq!(parse_packet(b"/a\0\0"), Err(OscError::Truncated));
        assert_eq!(parse_packet(b"/a\0\0i\0\0\0"), Err(OscError::BadTypeTag));
        assert_eq!(parse_packet(b"/a\0\0,d\0\0"), Err(OscError::BadTypeTag));
        assert_eq!(parse_packet(b"/a\0\0,i\0\0\0\0"), Err(OscError::Truncated));
        assert_eq!(parse_packet(b"/a\xff\0,\0\0\0"), Err(OscError::BadString));
        assert_eq!(
            parse_packet(&vec![0; MAX_PACKET_LEN + 1]),
            Err(OscError::TooLong)
        );
    }

    #[test]
    fn huge_counts_are_bounded() {
        // Type tag claiming 10_000 ints but carrying none.
        let mut b = b"/a\0\0".to_vec();
        let mut tags = b",".to_vec();
        tags.extend(std::iter::repeat_n(b'i', 10_000));
        tags.push(0);
        while !tags.len().is_multiple_of(4) {
            tags.push(0);
        }
        b.extend_from_slice(&tags);
        assert_eq!(parse_packet(&b), Err(OscError::TooMany));
        // A bundle element claiming a huge size.
        let mut b = bundle(&[]);
        b.extend_from_slice(&0xffff_fff0u32.to_be_bytes());
        let p = parse_packet(&b).unwrap();
        assert_eq!((p.messages.len(), p.dropped), (0, 1));
        // Many tiny elements are capped at MAX_MESSAGES.
        let one = message("/m", &[]);
        let many = bundle(&vec![one; 3000]);
        let p = parse_packet(&many).unwrap();
        assert_eq!(p.messages.len(), MAX_MESSAGES);
        assert_eq!(p.dropped, 3000 - MAX_MESSAGES);
    }

    #[test]
    fn deep_bundle_nesting_is_rejected() {
        let mut b = message("/x", &[]);
        for _ in 0..100 {
            b = bundle(&[b]);
        }
        let p = parse_packet(&b).unwrap();
        assert!(p.messages.is_empty());
    }

    /// Mutation test: truncate at every length, overwrite every byte, and
    /// fuzz with a deterministic PRNG. Only "does not panic" is asserted.
    #[test]
    fn mutations_never_panic() {
        let seeds = [
            message("/VMC/Ext/Blend/Val", &[s("jawOpen"), Arg::Float(0.5)]),
            bundle(&[
                message("/VMC/Ext/Blend/Val", &[s("A"), Arg::Float(1.0)]),
                message("/VMC/Ext/Blend/Apply", &[]),
                bundle(&[message("/k", &[Arg::Int(-1)])]),
            ]),
        ];
        let mut rng = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        for seed in &seeds {
            for len in 0..=seed.len() {
                let _ = parse_packet(&seed[..len]);
            }
            for i in 0..seed.len() {
                for v in [0x00, 0x01, 0x7f, 0x80, 0xff, b'/', b',', b'#'] {
                    let mut m = seed.clone();
                    m[i] = v;
                    let _ = parse_packet(&m);
                }
            }
            for _ in 0..20_000 {
                let mut m = seed.clone();
                for _ in 0..=(next() % 4) {
                    let i = (next() as usize) % m.len();
                    m[i] = next() as u8;
                }
                if next() % 4 == 0 {
                    m.truncate((next() as usize) % (m.len() + 1));
                }
                let _ = parse_packet(&m);
            }
        }
        for _ in 0..20_000 {
            let n = (next() % 200) as usize;
            let m: Vec<u8> = (0..n).map(|_| next() as u8).collect();
            let _ = parse_packet(&m);
        }
    }
}
