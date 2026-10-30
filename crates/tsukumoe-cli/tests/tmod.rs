// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The reference model (REQ-PERF-001) through the runtime format: a loaded
//! file evaluates to the same bits as the compiled model, and damaged files
//! are errors, not panics.

use std::panic::{AssertUnwindSafe, catch_unwind};

use tsukumoe_cli::generate::{Table, model};
use tsukumoe_core::compiled::CompiledModel;
use tsukumoe_core::eval::Frame;
use tsukumoe_core::tmod::{self, AlignedBuf, Tmod};

fn reference() -> CompiledModel<'static> {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    model(&table).unwrap().compile().unwrap()
}

fn bits(s: &[f32]) -> Vec<u32> {
    s.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn the_reference_model_round_trips_bit_for_bit_and_deterministically() {
    let c = reference();
    let bytes = tmod::write(&c, &[]).unwrap();
    assert_eq!(bytes, tmod::write(&c, &[]).unwrap());
    let buf = AlignedBuf::from_bytes(&bytes);
    let t = Tmod::parse(&buf).unwrap();

    let (mut a, mut b) = (Frame::new(&c), Frame::new(&t.model));
    for step in 0..6 {
        for (i, p) in c.parameters.iter().enumerate() {
            let v = p.min + (p.max - p.min) * (((step * 5 + i * 3) % 9) as f32 / 8.0);
            a.set_parameter(i, v);
            b.set_parameter(i, v);
        }
        a.advance(&c, 1.0 / 60.0);
        b.advance(&t.model, 1.0 / 60.0);
        assert_eq!(bits(a.positions()), bits(b.positions()), "step {step}");
        let (la, lb) = (a.draw_list(&c), b.draw_list(&t.model));
        assert_eq!(la.order(), lb.order());
        for m in 0..la.len() {
            let (x, y) = (la.drawable(m), lb.drawable(m));
            assert_eq!(
                bits(&[x.opacity, x.draw_order]),
                bits(&[y.opacity, y.draw_order])
            );
            assert_eq!((x.name, x.masks, x.indices), (y.name, y.masks, y.indices));
        }
    }
}

#[test]
fn a_damaged_reference_file_is_an_error_not_a_panic() {
    let bytes = tmod::write(&reference(), &[]).unwrap();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut accepted = 0;
    for i in 0..300 {
        let mut b = bytes.clone();
        match i % 3 {
            0 => b.truncate(next() as usize % bytes.len()),
            1 => {
                let at = next() as usize % bytes.len();
                b[at] = next() as u8;
            }
            _ => {
                let at = next() as usize % (bytes.len() - 3);
                b[at..at + 4].copy_from_slice(&[0xFF; 4]);
            }
        }
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let buf = AlignedBuf::from_bytes(&b);
            let Ok(t) = Tmod::parse(&buf) else {
                return false;
            };
            let mut f = Frame::new(&t.model);
            f.advance(&t.model, 0.02);
            let _ = f.draw_list(&t.model).drawables().count();
            true
        }));
        match outcome {
            Ok(ok) => accepted += usize::from(ok),
            Err(_) => panic!("panicked on mutation {i}"),
        }
    }
    assert!(accepted < 300);
}
