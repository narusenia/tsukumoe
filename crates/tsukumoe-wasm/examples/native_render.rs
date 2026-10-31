// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The native side of the browser check: renders a `.tmod` at its default
//! parameters and writes the premultiplied RGBA8 pixels (no header).
//! `native_render <in.tmod> <x0,y0,x1,y1> <width>x<height> <out.rgba>`

use tsukumoe_core::tmod::{AlignedBuf, Tmod};
use tsukumoe_gpu::{Headless, Texture};
use tsukumoe_runtime::Runtime;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, input, view, size, out] = &a[..] else {
        panic!("usage: native_render <in.tmod> <x0,y0,x1,y1> <WxH> <out.rgba>");
    };
    let bytes = std::fs::read(input).unwrap();
    let view: [f32; 4] = view
        .split(',')
        .map(|n| n.parse().unwrap())
        .collect::<Vec<f32>>()
        .try_into()
        .unwrap();
    let (w, h) = size.split_once('x').unwrap();
    let (w, h): (u32, u32) = (w.parse().unwrap(), h.parse().unwrap());

    let buf = AlignedBuf::from_bytes(&bytes);
    let tmod = Tmod::parse(&buf).unwrap();
    let textures: Vec<Texture> = (0..tmod.texture_count())
        .map(|i| Texture::from_png(tmod.texture(i).unwrap()).unwrap())
        .collect();
    let mut runtime = Runtime::from_bytes(&bytes).unwrap();
    runtime.update(0.0);
    let mut gpu = Headless::new().unwrap();
    gpu.set_textures(&textures).unwrap();
    let image = gpu.render(&runtime.draw_list(), view, w, h).unwrap();
    std::fs::write(out, image.rgba).unwrap();
}
