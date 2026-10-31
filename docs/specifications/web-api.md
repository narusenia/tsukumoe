# Tsukumoe Web API

Related: REQ-RT-003 / `c-abi.md` (the same runtime for C), `draw-list.md`, `rendering.md`

License of this document: CC-BY 4.0

Status: defined in M3-04. The implementation is the crate `tsukumoe-wasm` (one WebAssembly module with the runtime and the wgpu renderer, wrapped by wasm-bindgen) and the TypeScript package `packages/tsukumoe-web` (`@tsukumoe/web`). The package's `dist/index.d.ts` is the typed spelling of this document.

## 1. Shape

- `Model`: the playback side of a loaded `.tmod`: parameters, motions, expressions, automatic motion, `update`, the draw list, the embedded textures. It needs no graphics and runs in Node.
- `Player`: a `Model` drawn into a canvas (`createPlayer(canvas, tmod, options)`). Drawing is the reference renderer of `rendering.md`, compiled to WASM, over WebGPU or WebGL2.
- One module holds both, so the draw list is read in place (no copy between modules).
- The package ships two builds of the module, with and without WebAssembly SIMD, and picks one at run time (`WebAssembly.validate` on a module with a v128 instruction). `options.simd` forces one. The two give the same results.

## 2. Conventions

- Errors are thrown (or the promise rejects) as `Error` with a message for people; branch on the call that failed, not on the wording.
- Indices are non-negative integers; an out-of-range index is `false` (parameter calls) or an `Error` / `RangeError` (`mesh(i)`, motion layers).
- Names are UTF-8 strings. Parameters, motions, expressions and meshes are listed in file order.
- `free()` releases the native memory; the object cannot be used afterwards. Garbage collection does not free it.
- `update(dt)` is in seconds; a negative or non-finite `dt` counts as 0 and one over `MAX_DT` (10 s) is cut, as in `tsukumoe-runtime`.

## 3. Backends

`createPlayer` tries WebGPU first: the adapter and device are requested before the canvas gets a context, so when the browser has no WebGPU adapter the same canvas is used for WebGL2. `options.backend` is `"auto"` (default), `"webgpu"` or `"webgl2"`; a forced backend that is not available rejects. `player.backend` is `"BrowserWebGpu"` or `"Gl"`. Rendering rules and results are the same on both; the pixel comparison against the native renderer is exact in M3-04's measurements.

The canvas is drawn with premultiplied alpha, so transparent parts show what is behind the canvas.

## 4. Draw list

`model.drawList` exposes the draw list of the last `update` as typed arrays over the module's memory. Nothing is copied.

| Property | Type | Content |
|---|---|---|
| `meshCount`, `vertexCount`, `indexCount` | number | sizes |
| `order` | `Uint32Array` (`meshCount`) | mesh indices, back to front |
| `orderChanged` | boolean | the order differs from the previous update |
| `positions` | `Float32Array` (`2 * vertexCount`) | `x, y` of all meshes in mesh order, drawing space (y up) |
| `uvs` | `Float32Array` (`2 * vertexCount`) | `u, v` (`v = 0` is the top row) |
| `indices` | `Uint32Array` (`indexCount`) | triangle lists, local to each mesh |
| `meshTable` | `Float32Array` (`meshCount * meshStride`) | per-mesh attributes, below |
| `mesh(i)` | `MeshInfo` | one decoded row (and its `masks`) |

`meshTable` rows (`meshStride` = 20; integers are exact in `f32`): texture, blend mode (0 normal, 1 additive, 2 multiply), double sided, vertex start, vertex count, index start, index count, mask start, mask count, mask inverted, opacity, draw order, multiply r, g, b, screen r, g, b, geometry changed, style changed. A mesh's vertices are `positions[2 * vertexStart ...]` for `vertexCount` vertices and its indices `indices[indexStart ...]` for `indexCount` entries; its masks are `mask count` mesh indices (`MeshInfo.masks`). The meanings are those of `draw-list.md`.

### Lifetime of the arrays

- The arrays are valid until the next call into the module that may grow its memory (loading a model or creating a player does; `update`, parameter calls and `render` do not).
- When the memory grows, the old `ArrayBuffer` is detached and every view over it becomes empty. The package makes a view anew on every property read, from the current `memory.buffer`, so reading `model.drawList.positions` after every `update` is always safe. A view kept across calls is not.
- `update` rewrites the arrays in place; copy what must outlive the next update.
- Textures are not in the draw list: `model.textures` copies the PNG files embedded in the `.tmod` (the same order as `texture` indices). Decoding and premultiplying them is the renderer's job (`rendering.md`).

## 5. API

`Model`: `Model.load(tmod, options)`, `parameters` (`index`, `name`, `min`, `max`, `default`), `parameterIndex(name)`, `setParameter(index, value)`, `clearParameter(index)`, `parameterValues`, `motionNames`, `expressionNames`, `playMotion(name, layer, priority)`, `stopMotion(layer)`, `isMotionPlaying(layer)`, `playExpression(name)`, `stopExpression(name)`, `setBlink(left, right, interval)`, `setBreath(parameter, cycle, amplitude, offset)`, `clearAuto()`, `update(dt)`, `textures`, `meshNames`, `bounds`, `drawList`, `free()`. Semantics are those of `tsukumoe-runtime` (`runtime-api.md`): a held parameter wins over motions, expressions and automatic motion; a motion of a higher priority is not interrupted (`playMotion` returns `false`).

`Player`: `model`, `backend`, `timings` (milliseconds: PNG decode, instance, adapter, device, pipelines, texture upload), `update(dt)`, `render()`, `setView(x0, y0, x1, y1)`, `fitView()`, `resize(width, height)`, `readPixels(width, height)` (premultiplied RGBA8, rows top to bottom, `width * 4` a multiple of 256), `free()`.

## 6. Size

Each distributed `.wasm` must stay within 1 MiB gzipped (WebGPU, WebGL2, glue and PNG decoding included). The build fails otherwise (`scripts/check-wasm-size.sh`).
