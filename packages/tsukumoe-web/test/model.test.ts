// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// The playback side in Node, without graphics. The fixture is written by
// `cargo run -p tsukumoe-wasm --example web_fixture -- test/fixture.tmod`.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Model, supportsSimd } from "../src/index.ts";

const tmod = new Uint8Array(readFileSync(new URL("./fixture.tmod", import.meta.url)));
const load = (simd = true) =>
  Model.load(tmod, {
    simd,
    wasm: readFileSync(new URL(`../src/wasm/${simd ? "simd" : "nosimd"}/tsukumoe_bg.wasm`, import.meta.url)),
  });

test("parameters carry names, ranges and defaults", async () => {
  const m = await load();
  assert.deepEqual(
    m.parameters.map((p) => [p.index, p.name, p.min, p.max, p.default]),
    [
      [0, "P", 0, 1, 0],
      [1, "Q", -1, 1, 0],
      [2, "EyeL", 0, 1, 1],
      [3, "EyeR", 0, 1, 1],
      [4, "Breath", 0, 1, 0],
    ],
  );
  assert.equal(m.parameterIndex("Q"), 1);
  assert.equal(m.parameterIndex("nope"), undefined);
  m.free();
});

test("a held parameter wins until it is cleared", async () => {
  const m = await load();
  assert.equal(m.setParameter(1, 0.25), true);
  assert.equal(m.setParameter(99, 1), false);
  m.update(0.1);
  assert.equal(m.parameterValues[1], 0.25);
  assert.equal(m.clearParameter(1), true);
  m.update(0.1);
  assert.equal(m.parameterValues[1], 0);
  m.free();
});

test("a motion drives its parameter and ends", async () => {
  const m = await load();
  assert.deepEqual(m.motionNames, ["rise"]);
  assert.equal(m.playMotion("rise"), true);
  assert.equal(m.isMotionPlaying(), true);
  m.update(0.5);
  assert.ok(Math.abs((m.parameterValues[0] ?? NaN) - 0.5) < 1e-3, `P = ${m.parameterValues[0]}`);
  m.update(2);
  assert.equal(m.isMotionPlaying(), false);
  assert.throws(() => m.playMotion("nope"), Error);
  assert.throws(() => m.playMotion("rise", 999), Error);
  m.free();
});

test("an expression overwrites its parameter", async () => {
  const m = await load();
  assert.deepEqual(m.expressionNames, ["half_q"]);
  m.playExpression("half_q");
  m.update(0.1);
  assert.equal(m.parameterValues[1], 0.5);
  m.stopExpression("half_q");
  assert.throws(() => m.playExpression("nope"), Error);
  m.free();
});

test("automatic motion moves the breath parameter and turns off", async () => {
  const m = await load();
  m.setBreath("Breath", 2, 0.5, 0.5);
  m.update(0.5);
  assert.ok(Math.abs((m.parameterValues[4] ?? NaN) - 1) < 1e-3, `Breath = ${m.parameterValues[4]}`);
  m.clearAuto();
  m.update(0.1);
  assert.equal(m.parameterValues[4], 0);
  assert.throws(() => m.setBreath("nope", 2, 1), Error);
  m.free();
});

test("the draw list is typed arrays over the module's memory", async () => {
  const m = await load();
  const d = m.drawList;
  assert.equal(d.meshCount, 3);
  assert.equal(d.vertexCount, 12);
  assert.equal(d.indexCount, 18);
  assert.equal(d.positions.length, 24);
  assert.equal(d.uvs.length, 24);
  assert.deepEqual([...d.order].sort(), [0, 1, 2]);
  assert.ok(d.positions.every(Number.isFinite));
  assert.ok(d.indices.every((i) => i < 4), "indices are local to each mesh");
  assert.equal(d.positions.buffer, m.wasm.memory.buffer);
  assert.equal(d.meshTable.length, 3 * d.meshStride);
  assert.deepEqual(m.bounds, [0, 0, 1, 1]);

  const [mask, a, b] = [d.mesh(0), d.mesh(1), d.mesh(2)];
  assert.deepEqual([mask.name, a.name, b.name], ["mask", "a", "b"]);
  assert.deepEqual([mask.texture, a.texture, b.texture], [1, 0, 2]);
  assert.equal(mask.opacity, 0);
  assert.deepEqual([...a.masks], [0]);
  assert.deepEqual([...b.masks], [0]);
  assert.equal(a.maskInverted, false);
  assert.equal(b.maskInverted, true);
  assert.ok(Math.abs(b.opacity - 0.8) < 1e-6);
  assert.deepEqual([a.vertexStart, a.vertexCount, a.indexStart, a.indexCount], [4, 4, 6, 6]);
  assert.equal(a.blendMode, "normal");
  assert.throws(() => d.mesh(3), RangeError);

  const pngs = m.textures;
  assert.equal(pngs.length, 3);
  assert.deepEqual([...(pngs[0] ?? []).slice(0, 4)], [0x89, 0x50, 0x4e, 0x47]);
  m.free();
});

test("views are made again after the memory grows", async () => {
  const m = await load();
  const d = m.drawList;
  const before = [...d.positions];
  const stale = d.positions;
  m.wasm.memory.grow(1);
  assert.equal(stale.length, 0, "a view over the old buffer is detached");
  assert.deepEqual([...d.positions], before);
  assert.equal(d.positions.buffer, m.wasm.memory.buffer);
  m.free();
});

test("the build without SIMD gives the same result", async () => {
  assert.equal(typeof supportsSimd(), "boolean");
  const [x, y] = [await load(true), await load(false)];
  for (const m of [x, y]) {
    m.playMotion("rise");
    m.update(0.25);
  }
  assert.deepEqual([...x.parameterValues], [...y.parameterValues]);
  assert.deepEqual([...x.drawList.positions], [...y.drawList.positions]);
  x.free();
  y.free();
});

test("a broken file is an Error", async () => {
  await assert.rejects(() => Model.load(new Uint8Array([1, 2, 3]), { simd: true }), Error);
});
