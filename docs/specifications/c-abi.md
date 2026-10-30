# Tsukumoe C ABI

Related: REQ-RT-002 / `file-formats.md` (`.tmod`), `draw-list.md`, `rendering.md`

License of this document: CC-BY 4.0

Status: defined in M3-03. The implementation is the crate `tsukumoe-ffi`; the header `crates/tsukumoe-ffi/include/tsukumoe.h` is generated from it (cbindgen) and a test fails when the committed header differs from the generated one. This document is the contract; the header is its C spelling.

The C ABI exposes the playback runtime: load a `.tmod`, set parameters, play motions and expressions, run automatic motion, advance time and read the draw list. It does not edit models.

## 1. Conventions

- Every symbol starts with `tsukumoe_` (functions), `Tsukumoe` (types) or `TSUKUMOE_` (constants).
- Integers have fixed widths (`uint32_t`, `int32_t`, `uint64_t`); sizes of caller buffers are `size_t`; floats are IEEE 754 `float`; strings are NUL-terminated UTF-8; booleans in structs the library fills are C `bool`.
- Counts and indices are `uint32_t`. Every array the library returns holds fewer than 2^32 elements.
- Every function returns `TsukumoeStatus`, except `tsukumoe_abi_version` and `tsukumoe_last_error_message`, which cannot fail.
- Outputs are written through pointer arguments. On a failure the outputs are not written, except that `tsukumoe_runtime_load` sets `*out` to NULL first.
- The library never takes ownership of a pointer the caller passes, and the caller never frees a pointer the library returns.
- Structs the library fills (`TsukumoeParameterInfo`, `TsukumoeDrawable`, `TsukumoeDrawBuffers`) are allocated by the caller and change only with a major version. A struct the caller fills and that may grow (`TsukumoeAutoConfig`) starts with its size (section 2).
- Enumeration values the caller passes are plain `int32_t` and are checked; an unknown value is `OUT_OF_RANGE`, never undefined behavior.

## 2. Versions

`tsukumoe_abi_version()` returns `(major << 16) | minor`; the constants `TSUKUMOE_ABI_MAJOR` and `TSUKUMOE_ABI_MINOR` give the version of the header a program was compiled with. This document describes **1.0**.

- A minor version only adds: functions, constants, enumeration values, and fields at the end of a caller-filled struct.
- A major version may change or remove anything.
- A program compiled against major `M` works with a library of major `M` and minor greater than or equal to the one it was compiled with.
- A struct the caller fills starts with `uint32_t size`, which the caller sets to `sizeof` of the struct as the program's header defines it. A library that knows fewer fields ignores the rest; a library that knows more uses the defaults of the fields past `size`. A `size` smaller than the first version's struct is `OUT_OF_RANGE`.

## 3. Status and errors

| `TsukumoeStatus` | Value | Meaning |
|---|---|---|
| `TSUKUMOE_STATUS_OK` | 0 | Done. |
| `TSUKUMOE_STATUS_NULL_ARGUMENT` | 1 | A required pointer is NULL. |
| `TSUKUMOE_STATUS_INVALID_DATA` | 2 | The `.tmod` cannot be loaded or is inconsistent; a string is not UTF-8. |
| `TSUKUMOE_STATUS_OUT_OF_RANGE` | 3 | An index, layer, size, enumeration value or number is out of range. |
| `TSUKUMOE_STATUS_NOT_FOUND` | 4 | No parameter, motion or expression has that name. |
| `TSUKUMOE_STATUS_INTERNAL` | 5 | A panic was caught at the boundary: a bug in the library. |

`tsukumoe_last_error_message()` returns the message of the last call on the **calling thread**: UTF-8, NUL-terminated, never NULL, empty after a call that returned `OK`. The pointer is valid until the next call into the library on the same thread. Messages are for people; their wording is not part of the ABI and programs must branch on the status.

No panic crosses the boundary: every function runs inside `catch_unwind` and a panic becomes `INTERNAL` with the panic message in the last error. After `INTERNAL` the library stays usable, but the runtime that was being called may be in an unspecified state; destroy it. The default panic hook still prints to standard error. A library built with `panic = "abort"` cannot do this and is not supported.

NULL and invalid pointers: a NULL pointer is detected and reported. A non-NULL pointer that does not point to what the function documents (a freed or misaligned runtime, a string without a NUL) is the caller's bug and is undefined behavior.

## 4. Ownership and lifetimes

- `tsukumoe_runtime_load` copies the `.tmod` bytes (the data need not be aligned): the caller may free the buffer as soon as the call returns.
- A runtime lives from a successful `tsukumoe_runtime_load` until `tsukumoe_runtime_destroy`. Destroying NULL is allowed. Using a runtime after destroy, or destroying it twice, is undefined behavior.
- **Until destroy**: parameter names, motion names, expression names, mesh names (`TsukumoeDrawable::name`), and the PNG bytes of textures.
- **Until the next `tsukumoe_runtime_update`, `play_motion`, `stop_motion`, `play_expression`, `stop_expression`, `set_auto`, `set_parameter`, `clear_parameter` or destroy on that runtime**: every pointer inside `TsukumoeDrawBuffers` and `TsukumoeDrawable` other than `name`. They point into the runtime's frame (nothing is copied); a program that wants to keep the geometry copies it. The contents change only at `tsukumoe_runtime_update`, but the library may reuse the storage after any call listed here.
- A pointer whose count is 0 may be non-NULL and must not be read.
- Strings the caller passes (names, `TsukumoeAutoConfig` strings) are read during the call only.
- Buffers returned by the library are read-only.

## 5. Threads

A runtime must be used from one thread at a time. Calls on different runtimes may run at the same time from different threads. Moving a runtime to another thread between calls is allowed. The functions that take no runtime (`tsukumoe_abi_version`, `tsukumoe_last_error_message`) may be called from any thread; the last error is kept per thread.

## 6. Types

- `TsukumoeRuntime`: opaque.
- `TsukumoePriority` (`int32_t` values): `IDLE` 0, `NORMAL` 1, `FORCE` 2.
- `TsukumoeBlendMode`: `NORMAL` 0, `ADDITIVE` 1, `MULTIPLY` 2 (see `rendering.md`).
- `TsukumoeParameterInfo`: `name`, `min`, `max`, `default_value`, `repeat`.
- `TsukumoeAutoConfig`: `size`, `flags` (`TSUKUMOE_AUTO_BLINK` 1, `TSUKUMOE_AUTO_BREATH` 2; 0 means no automatic motion), then the blink fields (`blink_seed`, `blink_left_eye`, `blink_right_eye`, `blink_interval`, `blink_close_time`, `blink_closed_time`, `blink_open_time`) and the breath fields (`breath_parameter`, `breath_cycle`, `breath_amplitude`, `breath_offset`). The meaning and limits of the values are those of the runtime's `BlinkConfig` and `BreathConfig`; the fields of a feature whose flag is clear are ignored. Parameter names are resolved when the call is made.
- `TsukumoeDrawable`: one mesh, with the fields of the draw list's per-drawable table (`draw-list.md`): `mesh` (mesh index), `name`, `texture`, `blend_mode`, `double_sided`, `positions` and `uvs` (`2 * vertex_count` floats each), `indices` (`index_count`, local to the mesh), `vertex_start`, `index_start`, `masks` (`mask_count` mesh indices), `mask_inverted`, `opacity`, `draw_order`, `multiply_color`, `screen_color`, `geometry_changed`, `style_changed`.
- `TsukumoeDrawBuffers`: the whole-model buffers of `draw-list.md`: `order` (`drawable_count` mesh indices, back to front), `order_changed`, `positions` and `uvs` (`2 * vertex_count` floats each, all meshes in mesh order), `indices` (`index_count`, local to each mesh).

## 7. Functions

| Function | Contract |
|---|---|
| `tsukumoe_runtime_load(data, len, out)` | Loads a `.tmod` of `len` bytes. `NULL_ARGUMENT` for NULL `data` or `out`; `INVALID_DATA` for anything the loader rejects (truncated, bad magic, unsupported major version, inconsistent model or playback data); `OUT_OF_RANGE` if `len` exceeds `isize::MAX`. |
| `tsukumoe_runtime_destroy(runtime)` | Releases the runtime. NULL is accepted. |
| `tsukumoe_runtime_parameter_count(runtime, out)` | Number of parameters. |
| `tsukumoe_runtime_parameter_info(runtime, index, out)` | Name, range, default and repeat flag of parameter `index`; `OUT_OF_RANGE` past the last. |
| `tsukumoe_runtime_parameter_index(runtime, name, out)` | Index of the parameter called `name`; `NOT_FOUND` if none; `INVALID_DATA` if `name` is not UTF-8. |
| `tsukumoe_runtime_set_parameter(runtime, index, value)` | Holds the parameter at `value` from the next update on, above motions, expressions and automatic motion (`Runtime::set_parameter`). |
| `tsukumoe_runtime_clear_parameter(runtime, index)` | Gives the parameter back to motions, expressions and automatic motion. |
| `tsukumoe_runtime_parameter_value(runtime, index, out)` | The value the parameter had at the last update, before drivers, physics and clamping (`Runtime::parameter_values`). |
| `tsukumoe_runtime_motion_count` / `_motion_name(runtime, index, out)` | Number and names of the motions. |
| `tsukumoe_runtime_expression_count` / `_expression_name(runtime, index, out)` | Number and names of the expressions. |
| `tsukumoe_runtime_play_motion(runtime, name, layer, priority, started)` | Plays the motion on `layer` (0 to 15) with a `TsukumoePriority`. `*started` (optional, may be NULL) is 0 when a motion of a higher priority is playing and keeps playing, else 1; that case is still `OK`. `NOT_FOUND` for an unknown motion, `OUT_OF_RANGE` for a bad layer or priority. |
| `tsukumoe_runtime_stop_motion(runtime, layer)` | Fades out the motion of `layer`. |
| `tsukumoe_runtime_is_motion_playing(runtime, layer, out)` | `*out` is 1 while the layer has a motion that has not been told to stop and has not ended, else 0. |
| `tsukumoe_runtime_play_expression(runtime, name)` / `_stop_expression` | Fades an expression in on top of the active ones / out. `NOT_FOUND` for an unknown name. |
| `tsukumoe_runtime_set_auto(runtime, config)` | Replaces the automatic motion and restarts its clock. `NOT_FOUND` for an unknown parameter name, `OUT_OF_RANGE` for a bad number or `size`. On an error nothing changes. |
| `tsukumoe_runtime_update(runtime, dt)` | Advances everything by `dt` seconds (negative and non-finite count as 0; more than 10 is cut to 10) and evaluates the frame, in the order of the `tsukumoe-runtime` crate documentation. |
| `tsukumoe_runtime_draw_buffers(runtime, out)` | The whole-model buffers of the last update. Before the first update the buffers hold zeros, so call `tsukumoe_runtime_update` (with `dt` 0 if no time should pass) first. |
| `tsukumoe_runtime_drawable(runtime, position, out)` | The drawable at `position` of the drawing order (0 is the back); `OUT_OF_RANGE` at or past `drawable_count`. |
| `tsukumoe_runtime_texture_count(runtime, out)` | Number of embedded textures (`.tmod` section 0x80). |
| `tsukumoe_runtime_texture(runtime, index, out_data, out_len)` | The PNG bytes of texture `index`, valid until destroy. Decoding is the caller's. |
| `tsukumoe_abi_version()` | Section 2. |
| `tsukumoe_last_error_message()` | Section 3. |

Results are the results of the `tsukumoe-runtime` crate: for the same `.tmod` and the same calls, the C API and the Rust API produce bit-identical floats. A test checks it.

## 8. Building and linking

`cargo build -p tsukumoe-ffi --release` builds a shared library (`libtsukumoe_ffi.so`, `libtsukumoe_ffi.dylib`, `tsukumoe_ffi.dll` with the import library `tsukumoe_ffi.dll.lib`) and a static library (`libtsukumoe_ffi.a`, `tsukumoe_ffi.lib`). Prefer the shared library: a static Rust library needs the platform's system libraries on the link line (`rustc --print native-static-libs` lists them), which differ per OS and toolchain.

The sample `crates/tsukumoe-ffi/examples/c/main.c` loads `samples/quad.tmod`, moves the parameter `P` and prints the vertex positions. The test `the_c_sample_builds_runs_and_prints_the_runtime_positions` builds it with the system C compiler (MSVC on Windows) and checks its output against the Rust API.
