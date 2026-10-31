// tsc does not copy the wasm-bindgen output: put it next to dist/index.js.
import { cpSync } from "node:fs";
cpSync("src/wasm", "dist/wasm", { recursive: true });
