# Tsukumoe Rendering Rules

Related: REQ-CORE-006, REQ-RT-005, REQ-RT-006, REQ-RT-007

License of this document: CC-BY 4.0

Status: defined in M1-09 (ADR-0024). The reference implementation is `tsukumoe-gpu`.

Any renderer that follows this document and consumes the draw list must produce output matching the reference renderer within the tolerance defined in section 5. Colour constants and mask details are checked against Cubism in M2.

## 1. Inputs and space

- The draw list (`draw-list.md`): vertex positions, UVs, indices, opacity, draw order, mask references (and inversion), multiply colour, screen colour, blend mode, double-sidedness.
- Drawing space is Y-up. A **view** rectangle `(x_min, y_min, x_max, y_max)` of drawing space is mapped onto the whole output image. The top row of the image is `y_max`.
- Textures are RGBA8. UV `(0, 0)` is the top-left corner of the image (`v = 0` is the top row). Samples are taken at pixel centres with bilinear filtering and clamped edges.
- The output is RGBA8, premultiplied, cleared to `(0, 0, 0, 0)`.

## 2. Draw order

Drawables are drawn in the order of the draw list (`order`, back to front). A drawable with opacity 0, or without triangles, draws nothing (a renderer may skip it). Back faces are culled unless `double_sided`; a front face is counter-clockwise in drawing space.

## 3. Masks

For a drawable with a non-empty `masks`:

1. The mask value `m` of a pixel is the **maximum over the mask meshes** of the texture alpha of that mesh at the pixel (0 where the mesh does not cover it). Opacity and colours of a mask mesh are ignored, and its own `masks` are ignored (one level). Back-face culling follows the mask mesh's own `double_sided`.
2. The factor is `m`, or `1 - m` when `mask_inverted` (the inversion applies once to the whole set).
3. The factor multiplies the drawable's colour **and** alpha after opacity (section 4).

A drawable with empty `masks` is not clipped, and `mask_inverted` has no effect on it.

## 4. Color and blending

- Textures are premultiplied alpha: on load, `rgb ← rgb · a`, on the stored (non-linear) values. No sRGB decode; every operation below works on the stored values.
- Let `t = (t_rgb, t_a)` be the premultiplied sample, `M` the multiply colour, `S` the screen colour, `o` the opacity, `k` the mask factor (1 without masks).

```
c = t_rgb · M                      multiply
c = c + S · t_a − c · S            screen, weighted by the sample alpha
s = (c, t_a) · o · k               the premultiplied source
```

Blend with the destination `d` (premultiplied):

| Mode | Result |
|---|---|
| Normal | `s + d · (1 − s_a)` (all four channels) |
| Additive | `rgb = s_rgb + d_rgb`, `a = d_a` |
| Multiply | `rgb = s_rgb · d_rgb + d_rgb · (1 − s_a)`, `a = d_a` |

Results are clamped to `[0, 1]`.

## 5. Conformance tolerance

- Pixels: each channel of each pixel differs by at most 2/255 from the reference.
- Draw-list numbers (positions, UVs, opacity, colours): at most 1e-4 absolute.

The comparator is `tsukumoe parity <a> <b>` (`tsukumoe-cli`), which exits non-zero beyond these tolerances; the capture files it reads are specified in `draw-list.md` ("Capture format").
