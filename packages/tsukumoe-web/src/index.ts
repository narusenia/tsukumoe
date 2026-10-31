// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// The wasm-bindgen output of the two builds. They have the same API; only the
// code differs (WebAssembly SIMD or not).
import type * as Glue from "./wasm/simd/tsukumoe.js";

type Wasm = {
  glue: typeof Glue;
  memory: WebAssembly.Memory;
};

/** Which graphics API to draw with. `"auto"` is WebGPU, else WebGL2. */
export type Backend = "auto" | "webgpu" | "webgl2";

export interface LoadOptions {
  /** Force the SIMD (`true`) or the plain (`false`) build. Default: detect. */
  simd?: boolean;
  /**
   * Where the `.wasm` file is: a URL, bytes or a compiled module. Default: the
   * file next to this package's code, which bundlers and browsers resolve
   * through `import.meta.url`. Node needs bytes (`fs.readFileSync`).
   */
  wasm?: URL | string | BufferSource | WebAssembly.Module;
}

export interface PlayerOptions extends LoadOptions {
  backend?: Backend;
}

export interface ParameterInfo {
  index: number;
  name: string;
  min: number;
  max: number;
  default: number;
}

export type BlendMode = "normal" | "additive" | "multiply";

/** Attributes of one mesh, from the mesh table. */
export interface MeshInfo {
  mesh: number;
  name: string;
  /** Index into the model's textures. */
  texture: number;
  blendMode: BlendMode;
  doubleSided: boolean;
  /** First vertex of the mesh in `positions` and `uvs` (a float offset is twice this). */
  vertexStart: number;
  vertexCount: number;
  /** First index of the mesh in `indices`; the indices are local to the mesh. */
  indexStart: number;
  indexCount: number;
  /** Mesh indices of the meshes clipping this one. */
  masks: Uint32Array;
  maskInverted: boolean;
  /** Final opacity, parents included. */
  opacity: number;
  drawOrder: number;
  multiplyColor: [number, number, number];
  screenColor: [number, number, number];
  /** Positions differ from the previous update. */
  geometryChanged: boolean;
  /** Opacity, draw order or a color differs from the previous update. */
  styleChanged: boolean;
}

/**
 * The draw list of the last `update`, as typed arrays over the module's
 * memory. Nothing is copied. The arrays are valid until the next call into
 * the model or player that may grow the memory (`update` does not; loading
 * does); read them again after every `update` and do not keep them. Reading a
 * property always looks at the current `memory.buffer`, so a growth between
 * two frames is handled.
 */
export interface DrawList {
  readonly meshCount: number;
  readonly vertexCount: number;
  readonly indexCount: number;
  /** The drawing order differs from the previous update. */
  readonly orderChanged: boolean;
  /** Mesh indices back to front. */
  readonly order: Uint32Array;
  /** `x, y` per vertex of all meshes in mesh order, drawing space (y up). */
  readonly positions: Float32Array;
  /** `u, v` per vertex (`v = 0` is the top row of the texture). */
  readonly uvs: Float32Array;
  /** Triangle list of all meshes, local to each mesh's vertices. */
  readonly indices: Uint32Array;
  /** Per-mesh numbers, `meshStride` per mesh; `mesh(i)` decodes a row. */
  readonly meshTable: Float32Array;
  readonly meshStride: number;
  mesh(index: number): MeshInfo;
}

/** The playback side: parameters, motions, expressions, automatic motion and the draw list. It needs no graphics. */
export class Model {
  readonly #raw: Glue.Model;
  readonly #wasm: Wasm;
  #meshNames: string[] | undefined;

  private constructor(raw: Glue.Model, wasm: Wasm) {
    this.#raw = raw;
    this.#wasm = wasm;
  }

  /** Loads a `.tmod` and evaluates its first frame. */
  static async load(tmod: Uint8Array, options: LoadOptions = {}): Promise<Model> {
    const wasm = await loadWasm(options);
    return new Model(wasm.glue.Model.load(tmod), wasm);
  }

  /** @internal */
  get raw(): Glue.Model {
    return this.#raw;
  }

  /** @internal */
  get wasm(): Wasm {
    return this.#wasm;
  }

  get parameters(): ParameterInfo[] {
    const names = this.#raw.parameter_names();
    const ranges = this.#raw.parameter_ranges();
    return names.map((name, index) => ({
      index,
      name,
      min: ranges[index * 3] ?? 0,
      max: ranges[index * 3 + 1] ?? 0,
      default: ranges[index * 3 + 2] ?? 0,
    }));
  }

  /** The index of a parameter, `undefined` when there is none. */
  parameterIndex(name: string): number | undefined {
    return this.#raw.parameter_index(name);
  }

  /** Holds the parameter at `value` until `clearParameter`. False when `index` is out of range. */
  setParameter(index: number, value: number): boolean {
    return this.#raw.set_parameter(index, value);
  }

  /** Gives the parameter back to motions, expressions and automatic motion. */
  clearParameter(index: number): boolean {
    return this.#raw.clear_parameter(index);
  }

  /** The parameter values of the last update, before drivers and clamping. */
  get parameterValues(): Float32Array {
    return this.#raw.parameter_values();
  }

  get motionNames(): string[] {
    return this.#raw.motion_names();
  }

  get expressionNames(): string[] {
    return this.#raw.expression_names();
  }

  /**
   * Plays a motion on `layer`. Returns false when a motion of a higher
   * priority keeps playing. Throws for an unknown name or layer.
   */
  playMotion(name: string, layer = 0, priority: "idle" | "normal" | "force" = "normal"): boolean {
    return this.#raw.play_motion(name, layer, { idle: 0, normal: 1, force: 2 }[priority]);
  }

  stopMotion(layer = 0): void {
    this.#raw.stop_motion(layer);
  }

  isMotionPlaying(layer = 0): boolean {
    return this.#raw.is_motion_playing(layer);
  }

  playExpression(name: string): void {
    this.#raw.play_expression(name);
  }

  stopExpression(name: string): void {
    this.#raw.stop_expression(name);
  }

  /** Blinking: the two eye parameters are multiplied by the openness. `interval` is seconds between blinks. */
  setBlink(leftEye: string, rightEye: string, interval = 4): void {
    this.#raw.set_blink(leftEye, rightEye, interval);
  }

  /** Breathing: `offset + amplitude * sin(2 pi t / cycle)` is added to the parameter. */
  setBreath(parameter: string, cycle: number, amplitude: number, offset = 0): void {
    this.#raw.set_breath(parameter, cycle, amplitude, offset);
  }

  /** Turns all automatic motion off. */
  clearAuto(): void {
    this.#raw.clear_auto();
  }

  /** Advances by `dt` seconds and evaluates the frame. */
  update(dt: number): void {
    this.#raw.update(dt);
  }

  /** The PNG files embedded in the `.tmod`, in texture index order (copies). */
  get textures(): Uint8Array[] {
    const out: Uint8Array[] = [];
    for (let i = 0; i < this.#raw.texture_count(); i++) {
      const png = this.#raw.texture_png(i);
      if (png) out.push(png);
    }
    return out;
  }

  get meshNames(): string[] {
    return (this.#meshNames ??= this.#raw.mesh_names());
  }

  /** `[x0, y0, x1, y1]` around all vertices of the last update. */
  get bounds(): [number, number, number, number] {
    const b = this.#raw.bounds();
    return [b[0] ?? 0, b[1] ?? 0, b[2] ?? 0, b[3] ?? 0];
  }

  get drawList(): DrawList {
    const raw = this.#raw;
    const memory = this.#wasm.memory;
    const names = () => this.meshNames;
    // A view is made on every read, so it always sees the current buffer.
    const f32 = (ptr: number, len: number) => new Float32Array(memory.buffer, ptr, len);
    const u32 = (ptr: number, len: number) => new Uint32Array(memory.buffer, ptr, len);
    return {
      get meshCount() {
        return raw.mesh_count();
      },
      get vertexCount() {
        return raw.vertex_count();
      },
      get indexCount() {
        return raw.index_count();
      },
      get orderChanged() {
        return raw.order_changed();
      },
      get order() {
        return u32(raw.order_ptr(), raw.mesh_count());
      },
      get positions() {
        return f32(raw.positions_ptr(), raw.vertex_count() * 2);
      },
      get uvs() {
        return f32(raw.uvs_ptr(), raw.vertex_count() * 2);
      },
      get indices() {
        return u32(raw.indices_ptr(), raw.index_count());
      },
      get meshTable() {
        return f32(raw.mesh_table_ptr(), raw.mesh_count() * raw.mesh_stride());
      },
      get meshStride() {
        return raw.mesh_stride();
      },
      mesh(index: number): MeshInfo {
        const stride = raw.mesh_stride();
        if (!Number.isInteger(index) || index < 0 || index >= raw.mesh_count()) {
          throw new RangeError(`mesh ${index} is out of range`);
        }
        const t = f32(raw.mesh_table_ptr(), raw.mesh_count() * stride);
        const r = (k: number) => t[index * stride + k] ?? 0;
        return {
          mesh: index,
          name: names()[index] ?? "",
          texture: r(0),
          blendMode: (["normal", "additive", "multiply"] as const)[r(1)] ?? "normal",
          doubleSided: r(2) !== 0,
          vertexStart: r(3),
          vertexCount: r(4),
          indexStart: r(5),
          indexCount: r(6),
          masks: u32(raw.masks_ptr() + r(7) * 4, r(8)),
          maskInverted: r(9) !== 0,
          opacity: r(10),
          drawOrder: r(11),
          multiplyColor: [r(12), r(13), r(14)],
          screenColor: [r(15), r(16), r(17)],
          geometryChanged: r(18) !== 0,
          styleChanged: r(19) !== 0,
        };
      },
    };
  }

  /** Releases the model's memory. The object is unusable afterwards. */
  free(): void {
    this.#raw.free();
  }
}

/** A model drawn into a canvas. */
export class Player {
  readonly model: Model;
  readonly #screen: Glue.Screen;

  /** @internal */
  constructor(model: Model, screen: Glue.Screen) {
    this.model = model;
    this.#screen = screen;
  }

  /** `"BrowserWebGpu"` or `"Gl"` (WebGL2). */
  get backend(): string {
    return this.#screen.backend();
  }

  /** Milliseconds spent creating the player: PNG decode, instance, adapter, device, pipelines, texture upload. */
  get timings(): Record<"decode" | "instance" | "adapter" | "device" | "pipelines" | "textures", number> {
    const t = this.#screen.timings();
    return {
      decode: t[0] ?? 0,
      instance: t[1] ?? 0,
      adapter: t[2] ?? 0,
      device: t[3] ?? 0,
      pipelines: t[4] ?? 0,
      textures: t[5] ?? 0,
    };
  }

  update(dt: number): void {
    this.model.update(dt);
  }

  /** Draws the last update into the canvas. */
  render(): void {
    this.#screen.render(this.model.raw);
  }

  /** Shows the rectangle `[x0, y0, x1, y1]` of drawing space. */
  setView(x0: number, y0: number, x1: number, y1: number): void {
    this.#screen.set_view(x0, y0, x1, y1);
  }

  /** Fits the view to the model, centred, with the canvas's aspect ratio. */
  fitView(): void {
    this.#screen.fit_view(this.model.raw);
  }

  /** Sets the size of the canvas in pixels. */
  resize(width: number, height: number): void {
    this.#screen.resize(width, height);
  }

  /**
   * Draws into an off-screen target and returns its premultiplied RGBA8
   * pixels, rows top to bottom. `width * 4` must be a multiple of 256.
   */
  readPixels(width: number, height: number): Promise<Uint8Array> {
    return this.#screen.read_pixels(this.model.raw, width, height);
  }

  /** Releases the model and the graphics resources. */
  free(): void {
    this.#screen.free();
    this.model.free();
  }
}

/**
 * Loads a `.tmod` and draws it into `canvas`: WebGPU when the browser has it,
 * else WebGL2 (`options.backend` forces one). Rejects with an `Error` when
 * neither works.
 */
export async function createPlayer(
  canvas: HTMLCanvasElement,
  tmod: Uint8Array,
  options: PlayerOptions = {},
): Promise<Player> {
  const model = await Model.load(tmod, options);
  try {
    const screen = await model.wasm.glue.Screen.create(canvas, model.raw, options.backend ?? "auto");
    return new Player(model, screen);
  } catch (e) {
    model.free();
    throw e instanceof Error ? e : new Error(String(e));
  }
}

/** Whether this engine runs WebAssembly SIMD. */
export function supportsSimd(): boolean {
  // The smallest module that uses a v128 instruction.
  const bytes = [0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11];
  return typeof WebAssembly === "object" && WebAssembly.validate(new Uint8Array(bytes));
}

const loaded = new Map<boolean, Promise<Wasm>>();

/** Loads and starts the module once per variant. */
function loadWasm(options: LoadOptions): Promise<Wasm> {
  const simd = options.simd ?? supportsSimd();
  let p = loaded.get(simd);
  if (!p) {
    p = (async () => {
      const glue = (await (simd
        ? import("./wasm/simd/tsukumoe.js")
        : import("./wasm/nosimd/tsukumoe.js"))) as typeof Glue;
      const exports = await glue.default(options.wasm === undefined ? undefined : { module_or_path: options.wasm });
      return { glue, memory: exports.memory };
    })();
    // A failed load is tried again next time.
    p.catch(() => loaded.delete(simd));
    loaded.set(simd, p);
  }
  return p;
}
