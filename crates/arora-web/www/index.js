import init, { Engine, Registry } from "./pkg/arora_web.js";
import jsyaml from "https://cdn.jsdelivr.net/npm/js-yaml@4.1.0/+esm";

// ── hard-coded IDs for the built-in quick-call demo ──────────────────────────
const PING_FN_ID    = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6";
const SUCCEED_FN_ID = "00cd31a8-2cf4-48e6-a957-69a55de90424";
const COS_FN_ID     = "c13757cb-2311-4c93-abcc-cb12d6cbb859";
const COS_ANGLE_PARAM_ID = "6c2a157c-4235-47b0-bff3-1eeef3e5747d";

// ── one engine instance for the whole page ───────────────────────────────────
let engine = null;
let registry = null;
let testWasmLoaded = false;

const outEl     = document.getElementById("out");
const runBtn    = document.getElementById("run");
const angleEl   = document.getElementById("angle");
const moduleList = document.getElementById("modules-list");
const noModules  = document.getElementById("no-modules");

// The primitive types a header names by their well-known ids. They have no
// records, so the registry is given their names here.
const PRIMITIVES = [
  ["00000000-0000-0000-0000-000000000000", "unit"],
  ["00000000-0000-0000-0000-000000000001", "bool"],
  ["00000000-0000-0000-0000-000000000002", "i8"],
  ["00000000-0000-0000-0000-000000000003", "i16"],
  ["00000000-0000-0000-0000-000000000004", "i32"],
  ["00000000-0000-0000-0000-000000000005", "i64"],
  ["00000000-0000-0000-0000-000000000006", "u8"],
  ["00000000-0000-0000-0000-000000000007", "u16"],
  ["00000000-0000-0000-0000-000000000008", "u32"],
  ["00000000-0000-0000-0000-000000000009", "u64"],
  ["00000000-0000-0000-0000-00000000000a", "f32"],
  ["00000000-0000-0000-0000-00000000000b", "f64"],
  ["00000000-0000-0000-0000-00000000000c", "str"],
];

// ── type-reference rendering ─────────────────────────────────────────────────
// Renders a header's type ref, { kind: "scalar" | "array" | "option", id },
// naming the type from the registry.
function renderTypeRef(typeRef) {
  if (!typeRef) return "unit";
  const id = typeof typeRef === "string" ? typeRef : (typeRef.id ?? "?");
  const suffix = { array: "[]", option: "?" }[typeRef?.kind] ?? "";
  const name = registry?.resolveId(id) ?? id.slice(-8);
  return `${name}${suffix}`;
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

// ── load a module from its header's text + ArrayBuffer ───────────────────────
async function loadModule(headerText, wasmBuffer, statusEl) {
  try {
    // A header.json, or a header as YAML: JSON is YAML too.
    const header = jsyaml.load(headerText);
    const headerJson = JSON.stringify(header);
    // Two-step load: prepareModule compiles + instantiates asynchronously
    // (Chrome rejects both above 8 MB on the main thread), then
    // loadPreparedModule completes the load synchronously.
    await engine.prepareModule(headerJson, new Uint8Array(wasmBuffer));
    const id = engine.loadPreparedModule(headerJson);
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
  const headerFile = document.getElementById("header-file").files[0];
  const wasmFile = document.getElementById("wasm-file").files[0];
  const statusEl = document.getElementById("load-file-status");

  if (!headerFile || !wasmFile) {
    statusEl.className = "load-status error";
    statusEl.textContent = "✗ please select both a header and a WASM file";
    return;
  }

  const [headerText, wasmBuffer] = await Promise.all([
    headerFile.text(),
    wasmFile.arrayBuffer(),
  ]);

  await loadModule(headerText, wasmBuffer, statusEl);
});

// ── built-in modules: module directories serve.sh stages ─────────────────────
// Each is www/modules/<name>/: header.json, the guest, records/, and
// record-files.json listing the records, which a static server cannot list.

async function fetchOk(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
  return response;
}

// Give the registry the name and parent of each record in `dir`/records/. A
// record's id is its file's name, `<kind>/<id>[@<version>].yaml`.
async function registerRecords(dir) {
  const paths = await fetchOk(`${dir}/record-files.json`).then(r => r.json());
  const entries = await Promise.all(paths.map(async path => {
    const record = jsyaml.load(await fetchOk(`${dir}/records/${path}`).then(r => r.text()));
    const id = path.split("/").pop().replace(/\.yaml$/, "").split("@")[0];
    return { id, name: record.name, parent: record.parent };
  }));
  registry.loadRecordsJson(JSON.stringify(entries));
}

async function loadModuleDir(dir, guest, statusEl) {
  statusEl.textContent = "loading…";
  statusEl.className = "load-status";
  let headerText, wasmBuffer;
  try {
    [headerText, wasmBuffer] = await Promise.all([
      fetchOk(`${dir}/header.json`).then(r => r.text()),
      fetchOk(`${dir}/${guest}`).then(r => r.arrayBuffer()),
      registerRecords(dir),
    ]);
  } catch (e) {
    statusEl.className = "load-status error";
    statusEl.textContent = `✗ ${e}`;
    console.error(e);
    return null;
  }
  return loadModule(headerText, wasmBuffer, statusEl);
}

document.getElementById("load-test-wasm-btn").addEventListener("click", async () => {
  const statusEl = document.getElementById("load-builtin-status");
  const id = await loadModuleDir("./modules/test-rust-wasm", "test_rust_wasm.wasm", statusEl);
  if (id) {
    testWasmLoaded = true;
    runBtn.disabled = false;
  }
});

document.getElementById("load-bt-nodes-btn").addEventListener("click", async () => {
  const statusEl = document.getElementById("load-builtin-status");
  await loadModuleDir(
    "./modules/test-behavior-tree-nodes",
    "test_behavior_tree_nodes.wasm",
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
registry = new Registry();
registry.loadRecordsJson(JSON.stringify(PRIMITIVES.map(([id, name]) => ({ id, name }))));
