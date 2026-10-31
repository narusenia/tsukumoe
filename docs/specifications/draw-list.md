# Draw list

Related: REQ-CORE-006, REQ-CORE-008 / `rendering.md`

Status: implemented in M1-07 (`tsukumoe-core::draw`). Rendering rules (masks, blending, tolerance) are in `rendering.md` and are decided with the reference renderer in M1-09.

The draw list is the last output of a frame and the only input a renderer needs. It is a view onto the compiled model and the evaluated frame: reading it copies nothing.

## Layout

All buffers are contiguous, little-endian, and hold plain `f32` / `u32`, so a C ABI or WASM host can read them in place.

| Buffer | Type | Content |
|---|---|---|
| positions | `f32` | `x, y` per vertex in drawing space, all meshes in mesh order |
| uvs | `f32` | `u, v` per vertex, same vertex order |
| indices | `u32` | triangle list of all meshes in mesh order; **local to each mesh** (add no offset; a renderer that uploads one vertex buffer adds the mesh's `vertex_start` as base vertex) |
| order | `u32` | mesh indices, back to front |

## Per drawable

| Field | Meaning |
|---|---|
| `texture` | texture index |
| `blend_mode` | `Normal`, `Additive` or `Multiply` |
| `double_sided` | draw back faces |
| `vertex_start`, `index_start` | where the mesh's slices begin in the buffers above |
| `masks`, `mask_inverted` | mesh indices of the meshes clipping this one; inversion applies to the whole set |
| `opacity` | final: the mesh's own value times its parents' (see `evaluation.md`) |
| `draw_order` | the interpolated draw order value |
| `multiply_color`, `screen_color` | RGB |
| `geometry_changed` | positions differ from the previous frame |
| `style_changed` | opacity, draw order or a color differs from the previous frame |

The list also has `order_changed`: the back-to-front order differs from the previous frame.

## Draw order

Ascending `draw_order`; ties are broken by ascending mesh index. The order is therefore deterministic. (Cubism's tie-breaking is checked in M2.)

## Change flags

Compared exactly (bit equality of `f32`) with the previous call of `evaluate` on the same frame. The first frame reports everything as changed. A renderer may skip uploading a buffer whose flag is false; it must not skip drawing.

## Not in the draw list

Visibility culling (opacity 0), mask buffer generation and texture binding belong to the renderer.

## Capture format (cross-environment parity)

Every environment (native, WASM, Unity) writes its draw list for comparison (`docs/implementation/M3-runtime/parity.md`) as one JSON file, next to the rendered image. The file is a snapshot of one frame, so it holds no change flags.

```json
{ "drawables": [ { "mesh": 0, "texture": 1, "blend_mode": 0, "double_sided": false,
  "mask_inverted": false, "masks": [2], "opacity": 1.0, "draw_order": 500.0,
  "multiply_color": [1, 1, 1], "screen_color": [0, 0, 0],
  "positions": [0, 0, 1, 0], "uvs": [0, 1, 1, 1] } ] }
```

- `drawables` is in drawing order, back to front: element `i` is mesh `order[i]`.
- `mesh` is the mesh index; `texture` the texture index; `blend_mode` is `0` Normal, `1` Additive, `2` Multiply; `masks` are mesh indices.
- `opacity` is final (parents multiplied in), as in the table above; `draw_order`, `multiply_color` and `screen_color` are the evaluated values.
- `positions` and `uvs` are this mesh's own vertices, `x, y` and `u, v` interleaved (the slice starting at `vertex_start`), in drawing space as evaluated, before any view transform.
- Numbers are plain JSON numbers (`f32` values written so that they read back exactly); no `NaN` or infinity. Extra fields are ignored by the comparator.

The image next to it is premultiplied RGBA8 with no header, top row first, row by row. Its size and view are part of the case, not of the file.

Comparison (`tsukumoe parity <a> <b>`): `mesh`, `texture`, `blend_mode`, `double_sided`, `mask_inverted`, `masks`, the drawable count and the array lengths must be equal; all other numbers differ by at most 1e-4 absolute, pixels by at most 2/255 per channel (`rendering.md` section 5).
