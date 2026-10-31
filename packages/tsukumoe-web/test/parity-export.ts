// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// The WASM side of the parity test (docs/implementation/M3-runtime/parity.md):
// plays every case of tests/parity/cases on the WASM build in Node and writes
// `<out>/<case>.json`, the draw list in the Capture format of
// docs/specifications/draw-list.md. The steps are the same as in the native
// exporter (crates/tsukumoe-cli/examples/parity_export.rs).
// `node test/parity-export.ts <models-dir> <cases-dir> <out-dir>`; run it with
// tests/parity/native-vs-wasm.sh.

import { readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { basename, join } from "node:path";
import { Model } from "../src/index.ts";

interface Step {
  params?: Record<string, number>;
  motion?: string;
  expression?: string;
  dt: number;
  repeat?: number;
}
interface Case {
  model: string;
  steps: Step[];
}

const [models, cases, out] = process.argv.slice(2);
if (!models || !cases || !out) throw new Error("usage: parity-export.ts <models-dir> <cases-dir> <out-dir>");
mkdirSync(out, { recursive: true });
const wasm = readFileSync(new URL("../src/wasm/simd/tsukumoe_bg.wasm", import.meta.url));

for (const file of readdirSync(cases).filter((f) => f.endsWith(".json")).sort()) {
  const c: Case = JSON.parse(readFileSync(join(cases, file), "utf8"));
  const m = await Model.load(new Uint8Array(readFileSync(join(models, c.model))), { simd: true, wasm });
  for (const s of c.steps) {
    for (const [name, value] of Object.entries(s.params ?? {})) {
      const i = m.parameterIndex(name);
      if (i === undefined) throw new Error(`no parameter ${name}`);
      m.setParameter(i, value);
    }
    if (s.motion !== undefined) m.playMotion(s.motion, 0, "normal");
    if (s.expression !== undefined) m.playExpression(s.expression);
    for (let k = 0; k < (s.repeat ?? 1); k++) m.update(s.dt);
  }
  const d = m.drawList;
  const drawables = [...d.order].map((index) => {
    const x = d.mesh(index);
    return {
      mesh: index,
      texture: x.texture,
      blend_mode: ["normal", "additive", "multiply"].indexOf(x.blendMode),
      double_sided: x.doubleSided,
      mask_inverted: x.maskInverted,
      masks: [...x.masks],
      opacity: x.opacity,
      draw_order: x.drawOrder,
      multiply_color: x.multiplyColor,
      screen_color: x.screenColor,
      positions: [...d.positions.subarray(x.vertexStart * 2, (x.vertexStart + x.vertexCount) * 2)],
      uvs: [...d.uvs.subarray(x.vertexStart * 2, (x.vertexStart + x.vertexCount) * 2)],
    };
  });
  writeFileSync(join(out, `${basename(file, ".json")}.json`), JSON.stringify({ drawables }));
  m.free();
}
