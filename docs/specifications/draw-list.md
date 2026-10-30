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
