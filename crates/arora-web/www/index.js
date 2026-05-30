import init, { Engine } from "./pkg/arora_web.js";
import jsyaml from "https://cdn.jsdelivr.net/npm/js-yaml@4.1.0/+esm";

// ── hard-coded IDs for the built-in quick-call demo ──────────────────────────
const PING_FN_ID    = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6";
const SUCCEED_FN_ID = "00cd31a8-2cf4-48e6-a957-69a55de90424";
const COS_FN_ID     = "c13757cb-2311-4c93-abcc-cb12d6cbb859";
const COS_ANGLE_PARAM_ID = "6c2a157c-4235-47b0-bff3-1eeef3e5747d";

// ── one engine instance for the whole page ───────────────────────────────────
let engine = null;
let testWasmLoaded = false;

const outEl     = document.getElementById("out");
const runBtn    = document.getElementById("run");
const angleEl   = document.getElementById("angle");
const moduleList = document.getElementById("modules-list");
const noModules  = document.getElementById("no-modules");

// ── type-reference rendering ─────────────────────────────────────────────────
// Renders a YAML-derived type ref like { kind: "scalar", id: "f32" }
// or { kind: "array", id: "behavior_tree.Status" }.
function renderTypeRef(typeRef) {
  if (!typeRef) return "unit";
  const id = typeof typeRef === "string" ? typeRef : (typeRef.id ?? "?");
  const array = (typeRef.kind === "array") ? "[]" : "";
  // Shorten long dotted paths to just the last segment for display.
  const short = id.includes(".") ? id.split(".").pop() : id;
  return `${short}${array}`;
}

// ── module list rendering ─────────────────────────────────────────────────────
function renderModules() {
  if (!engine) return;
  const modules = JSON.parse(engine.listModules());

  if (modules.length === 0) {
    moduleList.innerHTML = '<div id="no-modules">No modules loaded yet.</div>';
    return;
  }

  moduleList.innerHTML = modules.map(m => {
    const exports = (m.exports ?? []).filter(e => e.type === "function");
    const fnItems = exports.map(fn => {
      const params = (fn.parameters ?? []).map(p => {
        const ty = renderTypeRef(p.type);
        const mut = p.mutable
          ? `<span class="fn-param-mut">&amp;mut </span>`
          : "";
        return `<span class="fn-param">${p.name}</span>: ${mut}${ty}`;
      }).join(", ");
      const ret = fn.ret
        ? ` <span style="color:#718096">→</span> <span class="fn-ret">${renderTypeRef(fn.ret)}</span>`
        : "";
      return `<li>
        <span class="fn-name">${fn.name}</span>(${params})${ret}
        <span class="fn-id">${fn.id}</span>
      </li>`;
    }).join("");

    return `<div class="module-card">
      <div class="mod-name">${m.name ?? "(unnamed)"}</div>
      <div class="mod-id">${m.id}</div>
      <ul class="fn-list">${fnItems || "<li><em style='color:#4a5568'>no exports</em></li>"}</ul>
    </div>`;
  }).join("");
}

// ── load a module from YAML text + ArrayBuffer ───────────────────────────────
async function loadModule(yamlText, wasmBuffer, statusEl) {
  try {
    const header = jsyaml.load(yamlText);
    const headerJson = JSON.stringify(header);
    const id = engine.loadModule(headerJson, new Uint8Array(wasmBuffer));
    statusEl.className = "load-status";
    statusEl.textContent = `✓ loaded ${header.name ?? id}`;
    renderModules();
    return id;
  } catch (e) {
    statusEl.className = "load-status error";
    statusEl.textContent = `✗ ${e}`;
    console.error(e);
    return null;
  }
}

// ── file-picker handler ───────────────────────────────────────────────────────
document.getElementById("load-file-btn").addEventListener("click", async () => {
  const yamlFile = document.getElementById("yaml-file").files[0];
  const wasmFile = document.getElementById("wasm-file").files[0];
  const statusEl = document.getElementById("load-file-status");

  if (!yamlFile || !wasmFile) {
    statusEl.className = "load-status error";
    statusEl.textContent = "✗ please select both a YAML and a WASM file";
    return;
  }

  const [yamlText, wasmBuffer] = await Promise.all([
    yamlFile.text(),
    wasmFile.arrayBuffer(),
  ]);

  await loadModule(yamlText, wasmBuffer, statusEl);
});

// ── built-in module loaders ───────────────────────────────────────────────────
async function fetchAndLoad(yamlPath, wasmPath, statusEl) {
  statusEl.textContent = "loading…";
  statusEl.className = "load-status";
  const [yamlText, wasmBuffer] = await Promise.all([
    fetch(yamlPath).then(r => r.text()),
    fetch(wasmPath).then(r => r.arrayBuffer()),
  ]);
  return loadModule(yamlText, wasmBuffer, statusEl);
}

document.getElementById("load-test-wasm-btn").addEventListener("click", async () => {
  const statusEl = document.getElementById("load-builtin-status");
  const id = await fetchAndLoad(
    "./modules/test-rust-wasm/module.yaml",
    "./modules/test-rust-wasm/test_rust_wasm.wasm",
    statusEl,
  );
  if (id) {
    testWasmLoaded = true;
    runBtn.disabled = false;
  }
});

document.getElementById("load-bt-nodes-btn").addEventListener("click", async () => {
  const statusEl = document.getElementById("load-builtin-status");
  await fetchAndLoad(
    "./modules/behavior-tree-nodes/module.yaml",
    "./modules/behavior-tree-nodes/behavior_tree_nodes.wasm",
    statusEl,
  );
});

// ── quick-call demo ───────────────────────────────────────────────────────────
function log(...parts) {
  outEl.textContent += parts
    .map(p => (typeof p === "string" ? p : JSON.stringify(p, null, 2)))
    .join(" ") + "\n";
}

runBtn.addEventListener("click", () => {
  outEl.textContent = "";
  try {
    const ping = engine.call(JSON.stringify({ id: PING_FN_ID, args: [] }));
    log("ping →", ping);

    const succeed = engine.call(JSON.stringify({ id: SUCCEED_FN_ID, args: [] }));
    log("succeed →", succeed);

    const angle = parseFloat(angleEl.value) || 0;
    const cos = engine.call(JSON.stringify({
      id: COS_FN_ID,
      args: [{ id: COS_ANGLE_PARAM_ID, value: { f32: angle } }],
    }));
    log(`cos(${angle}) →`, cos);
  } catch (e) {
    log("ERROR:", String(e));
    console.error(e);
  }
});

// ── boot ──────────────────────────────────────────────────────────────────────
await init();
engine = new Engine();
