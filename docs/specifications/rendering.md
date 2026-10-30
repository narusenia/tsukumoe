# Seimei Rendering Rules

Related: REQ-CORE-006, REQ-RT-005, REQ-RT-006, REQ-RT-007

License of this document: CC-BY 4.0

Status: outline. Defined in M1 together with the wgpu reference renderer.

Any renderer that follows this document and consumes the draw list must produce output matching the reference renderer within the tolerance defined in section 5.

## 1. Draw list

Per drawable: vertex positions, UVs, indices, opacity, draw order, mask references (and inversion), multiply color, screen color, blend mode, change flags.

## 2. Draw order

TODO: sort key and tie-breaking.

## 3. Masks

TODO: mask buffer generation, inverted masks, nested masks.

## 4. Color and blending

- Textures are premultiplied alpha.
- TODO: blend equations for each blend mode; how multiply and screen colors combine with the texture color.

## 5. Conformance tolerance

TODO: allowed per-pixel difference and allowed draw-list numeric difference.
