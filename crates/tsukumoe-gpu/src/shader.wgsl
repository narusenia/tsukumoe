// Rules: docs/specifications/rendering.md

struct Params {
    // x_min, y_min, 1 / width, 1 / height of the view in drawing space.
    view: vec4<f32>,
    multiply: vec4<f32>,
    screen: vec4<f32>,
    opacity: f32,
    // 0: no mask, 1: mask, 2: inverted mask.
    mask_mode: u32,
    pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(2) @binding(0) var mask: texture_2d<f32>;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.position = vec4<f32>((pos - params.view.xy) * params.view.zw * 2.0 - 1.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    var c = t.rgb * params.multiply.rgb;
    c = c + params.screen.rgb * t.a - c * params.screen.rgb;
    var k = 1.0;
    if params.mask_mode != 0u {
        let m = textureLoad(mask, vec2<i32>(in.position.xy), 0).r;
        k = select(m, 1.0 - m, params.mask_mode == 2u);
    }
    return vec4<f32>(c, t.a) * params.opacity * k;
}

// Mask generation: the texture alpha, combined by maximum blending.
@fragment
fn fs_mask(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(tex, samp, in.uv).a;
    return vec4<f32>(a, a, a, a);
}
