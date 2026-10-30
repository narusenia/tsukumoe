// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The C ABI of the playback runtime. The rules (lifetimes, threads, errors,
//! versions) are in `docs/specifications/c-abi.md`.
//!
//! Every function returns a [`TsukumoeStatus`] (except the two that cannot
//! fail), runs inside [`guard::guard`], and never panics across the boundary.
//! Pointer arguments must be NULL or valid as documented in the specification;
//! that is the contract of every `unsafe extern "C"` function below, so the
//! per-function safety sections are replaced by it.
#![allow(clippy::missing_safety_doc)]

mod guard;

use std::ffi::{CStr, CString, c_char};

use guard::{Failure, fail, guard};
use tsukumoe_core::tmod::{AlignedBuf, Tmod};
use tsukumoe_runtime::{AutoConfig, BlinkConfig, BreathConfig, MAX_LAYERS, Priority, Runtime};

pub use guard::{TsukumoeStatus, tsukumoe_last_error_message};

/// Incompatible changes raise the major version, additions the minor one.
pub const TSUKUMOE_ABI_MAJOR: u32 = 1;
pub const TSUKUMOE_ABI_MINOR: u32 = 0;

/// `(major << 16) | minor` of the ABI the library implements.
#[unsafe(no_mangle)]
pub extern "C" fn tsukumoe_abi_version() -> u32 {
    (TSUKUMOE_ABI_MAJOR << 16) | TSUKUMOE_ABI_MINOR
}

/// Motion priority, the `priority` argument of `tsukumoe_runtime_play_motion`.
#[repr(i32)]
pub enum TsukumoePriority {
    Idle = 0,
    Normal = 1,
    Force = 2,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsukumoeBlendMode {
    Normal = 0,
    Additive = 1,
    Multiply = 2,
}

/// `TsukumoeAutoConfig::flags`: run the blink / the breath.
pub const TSUKUMOE_AUTO_BLINK: u32 = 1;
pub const TSUKUMOE_AUTO_BREATH: u32 = 2;

/// Automatic motion. Set `size` to `sizeof(TsukumoeAutoConfig)`; a library
/// that knows more fields than `size` covers uses its defaults for them. The
/// strings are read during the call only.
#[repr(C)]
pub struct TsukumoeAutoConfig {
    pub size: u32,
    /// `TSUKUMOE_AUTO_*` bits. 0 turns automatic motion off.
    pub flags: u32,
    pub blink_seed: u64,
    pub blink_left_eye: *const c_char,
    pub blink_right_eye: *const c_char,
    pub blink_interval: f32,
    pub blink_close_time: f32,
    pub blink_closed_time: f32,
    pub blink_open_time: f32,
    pub breath_parameter: *const c_char,
    pub breath_cycle: f32,
    pub breath_amplitude: f32,
    pub breath_offset: f32,
}

#[repr(C)]
pub struct TsukumoeParameterInfo {
    /// Owned by the runtime, valid until `tsukumoe_runtime_destroy`.
    pub name: *const c_char,
    pub min: f32,
    pub max: f32,
    pub default_value: f32,
    pub repeat: bool,
}

/// One mesh to draw. The pointers are valid until the next call that changes
/// the runtime (update, set / clear parameter, play / stop, `set_auto`) or
/// `tsukumoe_runtime_destroy`, except `name`, which is valid until destroy.
/// A pointer whose count is 0 must not be read.
#[repr(C)]
pub struct TsukumoeDrawable {
    pub mesh: u32,
    pub name: *const c_char,
    pub texture: u32,
    pub blend_mode: TsukumoeBlendMode,
    pub double_sided: bool,
    /// `x, y` per vertex: `2 * vertex_count` floats.
    pub positions: *const f32,
    /// `u, v` per vertex: `2 * vertex_count` floats.
    pub uvs: *const f32,
    pub vertex_count: u32,
    /// Triangle list local to this mesh's vertices.
    pub indices: *const u32,
    pub index_count: u32,
    /// Where this mesh starts in the whole-model buffers.
    pub vertex_start: u32,
    pub index_start: u32,
    /// Mesh indices of the meshes clipping this one.
    pub masks: *const u32,
    pub mask_count: u32,
    pub mask_inverted: bool,
    pub opacity: f32,
    pub draw_order: f32,
    pub multiply_color: [f32; 3],
    pub screen_color: [f32; 3],
    pub geometry_changed: bool,
    pub style_changed: bool,
}

/// The whole-model buffers of the last update. Same validity as `TsukumoeDrawable`.
#[repr(C)]
pub struct TsukumoeDrawBuffers {
    /// Mesh indices back to front: `drawable_count` entries.
    pub order: *const u32,
    pub drawable_count: u32,
    pub order_changed: bool,
    /// `2 * vertex_count` floats, all meshes in mesh order.
    pub positions: *const f32,
    pub uvs: *const f32,
    pub vertex_count: u32,
    /// Local to each mesh, all meshes in mesh order.
    pub indices: *const u32,
    pub index_count: u32,
}

/// A loaded model with its playback state. Opaque.
pub struct TsukumoeRuntime {
    rt: Runtime<'static>,
    textures: Vec<Vec<u8>>,
    parameter_names: Vec<CString>,
    mesh_names: Vec<CString>,
    motion_names: Vec<CString>,
    expression_names: Vec<CString>,
}

fn cstrings<'a>(names: impl Iterator<Item = &'a str>) -> Vec<CString> {
    // An interior NUL would cut a name; none is expected.
    names
        .map(|n| CString::new(n.replace('\0', "\u{FFFD}")).unwrap_or_default())
        .collect()
}

impl TsukumoeRuntime {
    fn load(bytes: &[u8]) -> Result<Self, Failure> {
        let rt = Runtime::from_bytes(bytes)?;
        let buf = AlignedBuf::from_bytes(bytes);
        let tmod = Tmod::parse(&buf)?;
        let textures = (0..tmod.texture_count())
            .filter_map(|i| tmod.texture(i).map(<[u8]>::to_vec))
            .collect();
        let (model, pb) = (rt.model(), rt.playback());
        Ok(Self {
            textures,
            parameter_names: cstrings((0..model.parameters.len()).map(|i| model.parameter_name(i))),
            mesh_names: cstrings((0..model.meshes.len()).map(|i| model.mesh_name(i))),
            motion_names: cstrings((0..pb.motions.len()).map(|i| pb.motion_name(i))),
            expression_names: cstrings((0..pb.expressions.len()).map(|i| pb.expression_name(i))),
            rt,
        })
    }
}

unsafe fn handle<'a>(p: *const TsukumoeRuntime) -> Result<&'a TsukumoeRuntime, Failure> {
    unsafe { p.as_ref() }.ok_or_else(|| fail(TsukumoeStatus::NullArgument, "runtime is NULL"))
}

unsafe fn handle_mut<'a>(p: *mut TsukumoeRuntime) -> Result<&'a mut TsukumoeRuntime, Failure> {
    unsafe { p.as_mut() }.ok_or_else(|| fail(TsukumoeStatus::NullArgument, "runtime is NULL"))
}

unsafe fn outp<'a, T>(p: *mut T, what: &str) -> Result<&'a mut T, Failure> {
    unsafe { p.as_mut() }
        .ok_or_else(|| fail(TsukumoeStatus::NullArgument, format!("{what} is NULL")))
}

unsafe fn text<'a>(p: *const c_char, what: &str) -> Result<&'a str, Failure> {
    if p.is_null() {
        return Err(fail(
            TsukumoeStatus::NullArgument,
            format!("{what} is NULL"),
        ));
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| fail(TsukumoeStatus::InvalidData, format!("{what} is not UTF-8")))
}

fn index(i: u32, len: usize, what: &str) -> Result<usize, Failure> {
    if (i as usize) < len {
        Ok(i as usize)
    } else {
        Err(fail(
            TsukumoeStatus::OutOfRange,
            format!("{what} {i} is out of range (count {len})"),
        ))
    }
}

fn check_layer(l: u32) -> Result<usize, Failure> {
    index(l, MAX_LAYERS, "layer")
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Loads a `.tmod`. `data` needs no alignment and may be freed after the
/// call: the runtime keeps its own copy. On success `*out` is a new runtime
/// to be released with `tsukumoe_runtime_destroy`; on failure it is NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_load(
    data: *const u8,
    len: usize,
    out: *mut *mut TsukumoeRuntime,
) -> TsukumoeStatus {
    guard(|| {
        let out = unsafe { outp(out, "out")? };
        *out = std::ptr::null_mut();
        if data.is_null() {
            return Err(fail(TsukumoeStatus::NullArgument, "data is NULL"));
        }
        if len > isize::MAX as usize {
            return Err(fail(TsukumoeStatus::OutOfRange, "len is too large"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        *out = Box::into_raw(Box::new(TsukumoeRuntime::load(bytes)?));
        Ok(())
    })
}

/// Releases a runtime and everything it returned. NULL is accepted and does nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_destroy(runtime: *mut TsukumoeRuntime) -> TsukumoeStatus {
    guard(|| {
        if !runtime.is_null() {
            drop(unsafe { Box::from_raw(runtime) });
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_parameter_count(
    runtime: *const TsukumoeRuntime,
    out: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        *unsafe { outp(out, "out")? } = count(r.parameter_names.len());
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_parameter_info(
    runtime: *const TsukumoeRuntime,
    idx: u32,
    out: *mut TsukumoeParameterInfo,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out = unsafe { outp(out, "out")? };
        let i = index(idx, r.parameter_names.len(), "parameter")?;
        let p = &r.rt.model().parameters[i];
        *out = TsukumoeParameterInfo {
            name: r.parameter_names[i].as_ptr(),
            min: p.min,
            max: p.max,
            default_value: p.default,
            repeat: p.repeat != 0,
        };
        Ok(())
    })
}

/// Looks a parameter up by name; `NOT_FOUND` when there is none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_parameter_index(
    runtime: *const TsukumoeRuntime,
    name: *const c_char,
    out: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let name = unsafe { text(name, "name")? };
        let out = unsafe { outp(out, "out")? };
        let i = r.rt.parameter_index(name).ok_or_else(|| {
            fail(
                TsukumoeStatus::NotFound,
                format!("no parameter named '{name}'"),
            )
        })?;
        *out = count(i);
        Ok(())
    })
}

/// Holds a parameter at `value` from the next update on, above motions,
/// expressions and automatic motion, until `tsukumoe_runtime_clear_parameter`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_set_parameter(
    runtime: *mut TsukumoeRuntime,
    idx: u32,
    value: f32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle_mut(runtime)? };
        let i = index(idx, r.parameter_names.len(), "parameter")?;
        r.rt.set_parameter(i, value);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_clear_parameter(
    runtime: *mut TsukumoeRuntime,
    idx: u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle_mut(runtime)? };
        let i = index(idx, r.parameter_names.len(), "parameter")?;
        r.rt.clear_parameter(i);
        Ok(())
    })
}

/// The value a parameter had at the last update, as it went into the
/// evaluation (before drivers and physics, and before clamping).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_parameter_value(
    runtime: *const TsukumoeRuntime,
    idx: u32,
    out: *mut f32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out = unsafe { outp(out, "out")? };
        *out = r.rt.parameter_values()[index(idx, r.parameter_names.len(), "parameter")?];
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_motion_count(
    runtime: *const TsukumoeRuntime,
    out: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        *unsafe { outp(out, "out")? } = count(r.motion_names.len());
        Ok(())
    })
}

/// The name of motion `index`, valid until destroy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_motion_name(
    runtime: *const TsukumoeRuntime,
    idx: u32,
    out: *mut *const c_char,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out = unsafe { outp(out, "out")? };
        *out = r.motion_names[index(idx, r.motion_names.len(), "motion")?].as_ptr();
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_expression_count(
    runtime: *const TsukumoeRuntime,
    out: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        *unsafe { outp(out, "out")? } = count(r.expression_names.len());
        Ok(())
    })
}

/// The name of expression `index`, valid until destroy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_expression_name(
    runtime: *const TsukumoeRuntime,
    idx: u32,
    out: *mut *const c_char,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out = unsafe { outp(out, "out")? };
        *out = r.expression_names[index(idx, r.expression_names.len(), "expression")?].as_ptr();
        Ok(())
    })
}

/// Plays motion `name` on `layer` (below 16) with a
/// `TsukumoePriority`. `*started` (may be NULL) is 0 when a motion of a higher
/// priority is playing and keeps playing, else 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_play_motion(
    runtime: *mut TsukumoeRuntime,
    name: *const c_char,
    layer: u32,
    priority: i32,
    started: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle_mut(runtime)? };
        let name = unsafe { text(name, "name")? };
        let priority = match priority {
            0 => Priority::Idle,
            1 => Priority::Normal,
            2 => Priority::Force,
            p => {
                return Err(fail(
                    TsukumoeStatus::OutOfRange,
                    format!("priority {p} is not 0, 1 or 2"),
                ));
            }
        };
        let layer = check_layer(layer)?;
        let ok = r.rt.play_motion(name, layer, priority)?;
        if let Some(s) = unsafe { started.as_mut() } {
            *s = u32::from(ok);
        }
        Ok(())
    })
}

/// Fades out the motion of `layer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_stop_motion(
    runtime: *mut TsukumoeRuntime,
    layer: u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle_mut(runtime)? };
        r.rt.stop(check_layer(layer)?);
        Ok(())
    })
}

/// `*out` is 1 when `layer` has a motion that is still going, else 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_is_motion_playing(
    runtime: *const TsukumoeRuntime,
    layer: u32,
    out: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out = unsafe { outp(out, "out")? };
        *out = u32::from(r.rt.is_playing(check_layer(layer)?));
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_play_expression(
    runtime: *mut TsukumoeRuntime,
    name: *const c_char,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle_mut(runtime)? };
        r.rt.play_expression(unsafe { text(name, "name")? })?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_stop_expression(
    runtime: *mut TsukumoeRuntime,
    name: *const c_char,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle_mut(runtime)? };
        r.rt.stop_expression(unsafe { text(name, "name")? })?;
        Ok(())
    })
}

/// Replaces the automatic motion; its clock starts again. On an error nothing changes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_set_auto(
    runtime: *mut TsukumoeRuntime,
    config: *const TsukumoeAutoConfig,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle_mut(runtime)? };
        let c = unsafe { config.as_ref() }
            .ok_or_else(|| fail(TsukumoeStatus::NullArgument, "config is NULL"))?;
        if (c.size as usize) < std::mem::size_of::<TsukumoeAutoConfig>() {
            return Err(fail(
                TsukumoeStatus::OutOfRange,
                "config.size is smaller than sizeof(TsukumoeAutoConfig)",
            ));
        }
        let mut auto = AutoConfig::default();
        if c.flags & TSUKUMOE_AUTO_BLINK != 0 {
            let mut b = BlinkConfig::new(
                unsafe { text(c.blink_left_eye, "blink_left_eye")? },
                unsafe { text(c.blink_right_eye, "blink_right_eye")? },
            );
            b.interval = c.blink_interval;
            b.close_time = c.blink_close_time;
            b.closed_time = c.blink_closed_time;
            b.open_time = c.blink_open_time;
            b.seed = c.blink_seed;
            auto.blink = Some(b);
        }
        if c.flags & TSUKUMOE_AUTO_BREATH != 0 {
            auto.breath = Some(BreathConfig {
                parameter: unsafe { text(c.breath_parameter, "breath_parameter")? }.into(),
                cycle: c.breath_cycle,
                amplitude: c.breath_amplitude,
                offset: c.breath_offset,
            });
        }
        r.rt.set_auto(&auto)?;
        Ok(())
    })
}

/// Advances everything by `dt` seconds (negative and non-finite count as 0,
/// more than 10 is cut to 10) and evaluates the frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_update(
    runtime: *mut TsukumoeRuntime,
    dt: f32,
) -> TsukumoeStatus {
    guard(|| {
        unsafe { handle_mut(runtime)? }.rt.update(dt);
        Ok(())
    })
}

/// The whole-model buffers of the last update. Before the first update they
/// hold zeros: call `tsukumoe_runtime_update` first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_draw_buffers(
    runtime: *const TsukumoeRuntime,
    out: *mut TsukumoeDrawBuffers,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out = unsafe { outp(out, "out")? };
        let list = r.rt.draw_list();
        *out = TsukumoeDrawBuffers {
            order: list.order().as_ptr(),
            drawable_count: count(list.order().len()),
            order_changed: list.order_changed(),
            positions: list.positions().as_ptr(),
            uvs: list.uvs().as_ptr(),
            vertex_count: count(list.positions().len() / 2),
            indices: list.indices().as_ptr(),
            index_count: count(list.indices().len()),
        };
        Ok(())
    })
}

/// The drawable at `position` in the drawing order (0 is the back).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_drawable(
    runtime: *const TsukumoeRuntime,
    position: u32,
    out: *mut TsukumoeDrawable,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out = unsafe { outp(out, "out")? };
        let list = r.rt.draw_list();
        let mesh = list.order()[index(position, list.order().len(), "drawable")?] as usize;
        let d = list.drawable(mesh);
        *out = TsukumoeDrawable {
            mesh: count(mesh),
            name: r.mesh_names[mesh].as_ptr(),
            texture: d.texture,
            blend_mode: match d.blend_mode {
                tsukumoe_core::rig::BlendMode::Normal => TsukumoeBlendMode::Normal,
                tsukumoe_core::rig::BlendMode::Additive => TsukumoeBlendMode::Additive,
                tsukumoe_core::rig::BlendMode::Multiply => TsukumoeBlendMode::Multiply,
            },
            double_sided: d.double_sided,
            positions: d.positions.as_ptr(),
            uvs: d.uvs.as_ptr(),
            vertex_count: count(d.positions.len() / 2),
            indices: d.indices.as_ptr(),
            index_count: count(d.indices.len()),
            vertex_start: count(d.vertex_start),
            index_start: count(d.index_start),
            masks: d.masks.as_ptr(),
            mask_count: count(d.masks.len()),
            mask_inverted: d.mask_inverted,
            opacity: d.opacity,
            draw_order: d.draw_order,
            multiply_color: d.multiply_color,
            screen_color: d.screen_color,
            geometry_changed: d.geometry_changed,
            style_changed: d.style_changed,
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_texture_count(
    runtime: *const TsukumoeRuntime,
    out: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        *unsafe { outp(out, "out")? } = count(r.textures.len());
        Ok(())
    })
}

/// The PNG bytes of embedded texture `index`, valid until destroy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsukumoe_runtime_texture(
    runtime: *const TsukumoeRuntime,
    idx: u32,
    out_data: *mut *const u8,
    out_len: *mut u32,
) -> TsukumoeStatus {
    guard(|| {
        let r = unsafe { handle(runtime)? };
        let out_data = unsafe { outp(out_data, "out_data")? };
        let out_len = unsafe { outp(out_len, "out_len")? };
        let t = &r.textures[index(idx, r.textures.len(), "texture")?];
        *out_data = t.as_ptr();
        *out_len = count(t.len());
        Ok(())
    })
}
