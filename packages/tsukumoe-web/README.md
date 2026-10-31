# @tsukumoe/web

Plays [Tsukumoe](https://github.com/narusenia/tsukumoe) models (`.tmod`) in the browser. The runtime and the renderer are one WebAssembly module (Rust, `wgpu`): WebGPU where the browser has it, WebGL2 otherwise. The draw list is also exposed as typed arrays, so you can draw with your own renderer (PixiJS, Three.js, ...).

Status: not published to npm yet (`"private": true`).

## Use

```ts
import { createPlayer } from "@tsukumoe/web";

const tmod = new Uint8Array(await (await fetch("model.tmod")).arrayBuffer());
const player = await createPlayer(canvas, tmod); // { backend: "webgl2" } forces WebGL2

player.model.setParameter(player.model.parameterIndex("ParamAngleX")!, 15);
player.model.playMotion("idle", 0, "idle");
player.model.setBlink("ParamEyeLOpen", "ParamEyeROpen");

let last = performance.now();
requestAnimationFrame(function frame(now) {
  player.update((now - last) / 1000);
  player.render();
  last = now;
  requestAnimationFrame(frame);
});
```

`createPlayer` picks the build with WebAssembly SIMD when the engine supports it and the plain build otherwise (`{ simd: false }` forces the plain one). Set the canvas size before creating the player, or call `player.resize(width, height)`.

`Model` is the playback side alone and needs no graphics, so it also runs in Node:

```ts
import { readFileSync } from "node:fs";
import { Model } from "@tsukumoe/web";

const model = await Model.load(readFileSync("model.tmod"), {
  wasm: readFileSync("node_modules/@tsukumoe/web/dist/wasm/simd/tsukumoe_bg.wasm"),
  simd: true,
});
model.update(1 / 60);
```

## Reading the draw list yourself

`model.drawList` has typed arrays over the module's memory. Nothing is copied. They are valid until the next call that may grow the memory, so read them again after every `update` instead of keeping them (a property read always looks at the current `memory.buffer`).

```ts
model.update(dt);
const d = model.drawList;
for (const meshIndex of d.order) {                // back to front
  const m = d.mesh(meshIndex);
  if (m.opacity <= 0 || m.indexCount === 0) continue;
  const positions = d.positions.subarray(m.vertexStart * 2, (m.vertexStart + m.vertexCount) * 2);
  const uvs = d.uvs.subarray(m.vertexStart * 2, (m.vertexStart + m.vertexCount) * 2);
  const indices = d.indices.subarray(m.indexStart, m.indexStart + m.indexCount); // local to the mesh
  // texture m.texture is model.textures[m.texture] (a PNG file), blend mode m.blendMode,
  // clipping meshes m.masks (inverted: m.maskInverted), colors m.multiplyColor / m.screenColor.
  // Only re-upload positions when m.geometryChanged.
}
```

Drawing space is y up, `v = 0` is the top row of a texture, colors are multiplied by opacity (premultiplied). The rules a renderer must follow are in `docs/specifications/rendering.md` and `draw-list.md` of the repository.

## Build and test

From the repository root (needs `mise`):

```
mise run web:build   # wasm with and without SIMD, glue, wasm-opt, TypeScript, size check
mise run web:test    # tsc --noEmit and the Node tests
```

The two wasm files must each stay within 1 MiB gzipped; `web:build` fails otherwise.

## License

Apache-2.0 OR MIT.
