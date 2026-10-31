// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// Local server for the browser half of the parity test. Node stdlib only,
// 127.0.0.1 only. `node tests/parity/web-server.mjs <models-dir> <out-dir> [port]`
//   GET  /parity.html, /pkg/*  the page and packages/tsukumoe-web (built)
//   GET  /models/*             the .tmod files of <models-dir>
//   GET  /cases/<name>.json    the cases (and /cases/index.json: their names)
//   POST /rgba/<case>          writes the body to <out-dir>/<case>.rgba
//   POST /done                 prints the backend sent and exits
import { createServer } from "node:http";
import { readFileSync, readdirSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const [models, out, port = "8765"] = process.argv.slice(2);
if (!models || !out) throw new Error("usage: web-server.mjs <models-dir> <out-dir> [port]");
mkdirSync(out, { recursive: true });
const cases = join(here, "cases");
const types = { ".html": "text/html", ".js": "text/javascript", ".json": "application/json", ".wasm": "application/wasm" };

const roots = [
  ["/pkg/", join(here, "../../packages/tsukumoe-web")],
  ["/models/", models],
  ["/cases/", cases],
];

createServer((req, res) => {
  const url = new URL(req.url ?? "/", "http://127.0.0.1");
  const fail = (code) => {
    res.writeHead(code).end();
  };
  if (req.method === "POST") {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const body = Buffer.concat(chunks);
      if (url.pathname.startsWith("/rgba/") && /^[\w-]+$/.test(url.pathname.slice(6))) {
        writeFileSync(join(out, `${url.pathname.slice(6)}.rgba`), body);
        res.writeHead(204).end();
      } else if (url.pathname === "/done") {
        res.writeHead(204).end();
        console.log(body.toString());
        setTimeout(() => process.exit(0), 50);
      } else fail(404);
    });
    return;
  }
  if (url.pathname === "/parity.html") {
    res.writeHead(200, { "content-type": "text/html" }).end(readFileSync(join(here, "parity.html")));
    return;
  }
  if (url.pathname === "/cases/index.json") {
    const names = readdirSync(cases).filter((f) => f.endsWith(".json")).sort();
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(names));
    return;
  }
  for (const [prefix, dir] of roots) {
    if (!url.pathname.startsWith(prefix)) continue;
    const rel = normalize(url.pathname.slice(prefix.length));
    const file = join(dir, rel);
    if (rel.startsWith("..") || !existsSync(file)) return fail(404);
    const ext = file.slice(file.lastIndexOf("."));
    res.writeHead(200, { "content-type": types[ext] ?? "application/octet-stream" }).end(readFileSync(file));
    return;
  }
  fail(404);
}).listen(Number(port), "127.0.0.1", () => console.log(`parity server on http://127.0.0.1:${port}/parity.html`));
