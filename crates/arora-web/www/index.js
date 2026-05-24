// Minimal demo: load the test-rust-wasm guest into the browser-hosted
// arora engine and call a few of its functions. No bundler — relies on
// the JS shim emitted by `wasm-pack build --target web`.

import init, { Engine } from "./pkg/arora_web.js";

// jsyaml is loaded as a global in index.html (no bundler), but we keep
// it light: parse just enough YAML to grab a header. The arora-web
// Engine accepts JSON, so we parse YAML in JS and re-emit JSON.
import jsyaml from "https://cdn.jsdelivr.net/npm/js-yaml@4.1.0/+esm";

const PING_FN_ID = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6";
const SUCCEED_FN_ID = "00cd31a8-2cf4-48e6-a957-69a55de90424";
const COS_FN_ID = "c13757cb-2311-4c93-abcc-cb12d6cbb859";
const COS_ANGLE_PARAM_ID = "6c2a157c-4235-47b0-bff3-1eeef3e5747d";

const outEl = document.getElementById("out");
const angleEl = document.getElementById("angle");

function log(...parts) {
  outEl.textContent += parts.map((p) => (typeof p === "string" ? p : JSON.stringify(p, null, 2))).join(" ") + "\n";
}

async function run() {
  outEl.textContent = "";
  try {
    log("initializing wasm…");
    await init();

    log("fetching guest header + bytes…");
    const [headerYaml, wasmBytes] = await Promise.all([
      fetch("./test-rust-wasm/module.yaml").then((r) => r.text()),
      fetch("./test-rust-wasm/test_rust_wasm.wasm").then((r) => r.arrayBuffer()),
    ]);
    const header = jsyaml.load(headerYaml);
    const headerJson = JSON.stringify(header);

    const engine = new Engine();
    const moduleId = engine.loadModule(headerJson, new Uint8Array(wasmBytes));
    log("loaded module:", moduleId);

    const ping = engine.call(JSON.stringify({ id: PING_FN_ID, args: [] }));
    log("ping ->", ping);

    const succeed = engine.call(JSON.stringify({ id: SUCCEED_FN_ID, args: [] }));
    log("succeed ->", succeed);

    const angle = parseFloat(angleEl.value) || 0;
    const cosCall = {
      id: COS_FN_ID,
      args: [{ id: COS_ANGLE_PARAM_ID, value: { kind: "scalar", id: "f32", value: angle } }],
    };
    const cos = engine.call(JSON.stringify(cosCall));
    log(`cos(${angle}) ->`, cos);
  } catch (e) {
    log("ERROR:", String(e));
    console.error(e);
  }
}

document.getElementById("run").addEventListener("click", run);
