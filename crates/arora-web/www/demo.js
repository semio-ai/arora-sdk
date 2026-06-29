// demo.js — multi-tree behavior-tree demo for the Arora browser engine.

import init, { BehaviorTreeRunner } from "./pkg/arora_web.js";
import jsyaml from "https://cdn.jsdelivr.net/npm/js-yaml@4.1.0/+esm";

// ── UUIDs ─────────────────────────────────────────────────────────────────────

const FN = {
  SEQ:          "32246df6-ab5d-4f18-9221-23e28731de93",
  FALLBACK:     "bfa89a4e-c369-430e-be78-0dc07311391c",
  SUCCEED:      "6696f0bd-e781-40cd-aeb5-8dc616f810d2",
  FAIL:         "3abbbfb6-d00d-41eb-88bb-97874267eaf6",
  ADD:          "e4b0a2f3-6c7d-4e8f-9a0b-1c2d3e4f5a6b",
  COS:          "c13757cb-2311-4c93-abcc-cb12d6cbb859",
  IS_STR_SET:   "20ba3f0f-309e-4cd2-adfc-aca6cc432526",
  WAIT_STR_SET: "3180977c-25a1-458e-ab82-11f36c654518",
  REGEX_MATCH:  "8e3dbcc1-1a81-4cf6-a457-6e0c075456fd",
  UNSET_STR:    "7dce01ed-9818-4b7d-b45a-2e7fdece3633",
};

const PARAM = {
  // test-behavior-tree-nodes children parameter (seq/fallback/parallel)
  CHILDREN:         "5b6e9515-dbcc-411d-bee9-3d8cba5fedda",
  // _ret special out-parameter (captures function return value into a variable)
  RET:              "5f726574-0000-4000-8000-000000000000",
  // test-rust-wasm add
  ADD_A:            "a1b2c3d4-e5f6-4a8b-9c0d-e1f2a3b4c5d6",
  ADD_B:            "b2c3d4e5-f6a7-4b9c-8d1e-f2a3b4c5d6e7",
  // cos (imported from test-rust-wasm, re-exported via test-behavior-tree-nodes)
  COS_ANGLE:        "6c2a157c-4235-47b0-bff3-1eeef3e5747d",
  // is_str_set / wait_str_set value param
  IS_STR_SET_VALUE: "c4f1e72d-30fe-400b-a584-f08e93944026",
  WAIT_STR_VALUE:   "8f190079-e519-44d3-ac36-3bfc322e87eb",
  // regex_match
  REGEX_VALUE:      "3267f093-8a7f-4b77-b74c-3bd2e7ad40f9",
  REGEX_MATCHER:    "6702e02d-f6ba-4c5d-acab-9ade0a690afa",
  REGEX_FIRST_MATCH:"e8b71df7-2bb5-4498-8bc3-833c5bc8eadc",
  // unset_str
  UNSET_VAR:        "2c84bf0f-4ec2-41a4-83ee-3f92a53be79d",
};

// ── Variables ─────────────────────────────────────────────────────────────────
//
// Each tree declares its own variable set. The runner is recreated on tree
// switch so variables don't bleed across trees.

const TREES = {
  classic: {
    label: "Classic (fallback/seq)",
    variables: {},
    initial: {},
    nodes: [
      {
        id: "c0000001-0000-0000-0000-000000000000",
        function: FN.FALLBACK, label: "Fallback", kind: "control",
        children: ["c0000002-0000-0000-0000-000000000000",
                   "c0000003-0000-0000-0000-000000000000"],
      },
      {
        id: "c0000002-0000-0000-0000-000000000000",
        function: FN.SEQ, label: "Sequence", kind: "control",
        children: ["c0000004-0000-0000-0000-000000000000",
                   "c0000005-0000-0000-0000-000000000000",
                   "c0000006-0000-0000-0000-000000000000"],
      },
      {
        id: "c0000003-0000-0000-0000-000000000000",
        function: FN.SEQ, label: "Sequence", kind: "control",
        children: ["c0000007-0000-0000-0000-000000000000",
                   "c0000008-0000-0000-0000-000000000000"],
      },
      { id: "c0000004-0000-0000-0000-000000000000", function: FN.SUCCEED, label: "succeed", kind: "action" },
      { id: "c0000005-0000-0000-0000-000000000000", function: FN.FAIL,    label: "fail",    kind: "action" },
      { id: "c0000006-0000-0000-0000-000000000000", function: FN.SUCCEED, label: "succeed", kind: "action" },
      { id: "c0000007-0000-0000-0000-000000000000", function: FN.SUCCEED, label: "succeed", kind: "action" },
      { id: "c0000008-0000-0000-0000-000000000000", function: FN.SUCCEED, label: "succeed", kind: "action" },
    ],
  },

  addcos: {
    label: "Add + Cos",
    variables: {
      "aaaa0001-0000-0000-0000-000000000000": { name: "x",         type: "f32", init: 0.0 },
      "aaaa0002-0000-0000-0000-000000000000": { name: "cos(x)",    type: "f32", init: 1.0 },
    },
    initial: {
      "aaaa0001-0000-0000-0000-000000000000": { f32: 0.0 },
      "aaaa0002-0000-0000-0000-000000000000": { f32: 1.0 },
    },
    nodes: [
      {
        id: "20000001-0000-0000-0000-000000000000",
        function: FN.SEQ, label: "Sequence", kind: "control",
        children: ["20000002-0000-0000-0000-000000000000",
                   "20000003-0000-0000-0000-000000000000"],
      },
      {
        id: "20000002-0000-0000-0000-000000000000",
        function: FN.ADD, label: "add(x, 0.1)\n→ x", kind: "action",
        arguments: {
          [PARAM.ADD_A]: { variable_id: "aaaa0001-0000-0000-0000-000000000000" },
          [PARAM.ADD_B]: { value: { f32: 0.1 } },
          [PARAM.RET]:   { variable_id: "aaaa0001-0000-0000-0000-000000000000" },
        },
        // variable connections drawn as dashed lines
        reads:  ["aaaa0001-0000-0000-0000-000000000000"],
        writes: ["aaaa0001-0000-0000-0000-000000000000"],
      },
      {
        id: "20000003-0000-0000-0000-000000000000",
        function: FN.COS, label: "cos(x)\n→ cos(x)", kind: "action",
        arguments: {
          [PARAM.COS_ANGLE]: { variable_id: "aaaa0001-0000-0000-0000-000000000000" },
          [PARAM.RET]:       { variable_id: "aaaa0002-0000-0000-0000-000000000000" },
        },
        reads:  ["aaaa0001-0000-0000-0000-000000000000"],
        writes: ["aaaa0002-0000-0000-0000-000000000000"],
      },
    ],
  },

  mixed: {
    label: "Mixed (cross-branch vars)",
    variables: {
      "bbbb0001-0000-0000-0000-000000000000": { name: "x",      type: "f32", init: 0.0 },
      "bbbb0002-0000-0000-0000-000000000000": { name: "cos(x)", type: "f32", init: 0.0 },
    },
    initial: {
      "bbbb0001-0000-0000-0000-000000000000": { f32: 0.0 },
      "bbbb0002-0000-0000-0000-000000000000": { f32: 0.0 },
    },
    nodes: [
      {
        id: "30000001-0000-0000-0000-000000000000",
        function: FN.FALLBACK, label: "Fallback", kind: "control",
        children: ["30000002-0000-0000-0000-000000000000",
                   "30000003-0000-0000-0000-000000000000"],
      },
      {
        id: "30000002-0000-0000-0000-000000000000",
        function: FN.SEQ, label: "Sequence A", kind: "control",
        children: ["30000004-0000-0000-0000-000000000000",
                   "30000005-0000-0000-0000-000000000000"],
      },
      {
        id: "30000003-0000-0000-0000-000000000000",
        function: FN.SEQ, label: "Sequence B", kind: "control",
        children: ["30000006-0000-0000-0000-000000000000",
                   "30000007-0000-0000-0000-000000000000"],
      },
      {
        id: "30000004-0000-0000-0000-000000000000",
        function: FN.ADD, label: "add(x, 0.1)\n→ x", kind: "action",
        arguments: {
          [PARAM.ADD_A]: { variable_id: "bbbb0001-0000-0000-0000-000000000000" },
          [PARAM.ADD_B]: { value: { f32: 0.1 } },
          [PARAM.RET]:   { variable_id: "bbbb0001-0000-0000-0000-000000000000" },
        },
        reads:  ["bbbb0001-0000-0000-0000-000000000000"],
        writes: ["bbbb0001-0000-0000-0000-000000000000"],
      },
      { id: "30000005-0000-0000-0000-000000000000", function: FN.FAIL, label: "fail", kind: "action" },
      {
        id: "30000006-0000-0000-0000-000000000000",
        function: FN.COS, label: "cos(x)\n→ cos(x)", kind: "action",
        arguments: {
          [PARAM.COS_ANGLE]: { variable_id: "bbbb0001-0000-0000-0000-000000000000" },
          [PARAM.RET]:       { variable_id: "bbbb0002-0000-0000-0000-000000000000" },
        },
        reads:  ["bbbb0001-0000-0000-0000-000000000000"],
        writes: ["bbbb0002-0000-0000-0000-000000000000"],
      },
      { id: "30000007-0000-0000-0000-000000000000", function: FN.SUCCEED, label: "succeed", kind: "action" },
    ],
  },

  strings: {
    label: "String (wait + regex)",
    variables: {
      "cccc0001-0000-0000-0000-000000000000": { name: "message",     type: "string", init: "" },
      "cccc0002-0000-0000-0000-000000000000": { name: "first_match", type: "string", init: "" },
    },
    initial: {
      "cccc0001-0000-0000-0000-000000000000": { str: "" },
      "cccc0002-0000-0000-0000-000000000000": { str: "" },
    },
    nodes: [
      {
        id: "40000001-0000-0000-0000-000000000000",
        function: FN.SEQ, label: "Sequence", kind: "control",
        children: ["40000002-0000-0000-0000-000000000000",
                   "40000003-0000-0000-0000-000000000000",
                   "40000004-0000-0000-0000-000000000000"],
      },
      {
        id: "40000002-0000-0000-0000-000000000000",
        function: FN.WAIT_STR_SET, label: "wait_str_set\n(message)", kind: "action",
        arguments: {
          [PARAM.WAIT_STR_VALUE]: { variable_id: "cccc0001-0000-0000-0000-000000000000" },
        },
        reads: ["cccc0001-0000-0000-0000-000000000000"],
      },
      {
        id: "40000003-0000-0000-0000-000000000000",
        function: FN.REGEX_MATCH, label: "regex_match\n(message, hello.*)", kind: "action",
        arguments: {
          [PARAM.REGEX_VALUE]:       { variable_id: "cccc0001-0000-0000-0000-000000000000" },
          [PARAM.REGEX_MATCHER]:     { value: { str: "hello.*" } },
          [PARAM.REGEX_FIRST_MATCH]: { variable_id: "cccc0002-0000-0000-0000-000000000000" },
        },
        reads:  ["cccc0001-0000-0000-0000-000000000000"],
        writes: ["cccc0002-0000-0000-0000-000000000000"],
      },
      {
        id: "40000004-0000-0000-0000-000000000000",
        function: FN.SUCCEED, label: "succeed", kind: "action",
      },
    ],
  },
};

// ── SVG layout ────────────────────────────────────────────────────────────────

const SVG_W = 700;
const SVG_H = 280;
const LEVEL_H = 110;
const NODE_W = 150;
const NODE_H = 44;

function buildNodeMap(nodes) {
  const map = new Map();
  for (const n of nodes) map.set(n.id, { ...n, x: 0, y: 0 });
  return map;
}

function layout(nodeId, map, x, y, availableWidth) {
  const node = map.get(nodeId);
  node.x = x;
  node.y = y;
  if (!node.children || node.children.length === 0) return;
  const childW = availableWidth / node.children.length;
  let cx = x - availableWidth / 2 + childW / 2;
  for (const cid of node.children) {
    layout(cid, map, cx, y + LEVEL_H, childW);
    cx += childW;
  }
}

function renderSVG(map, varMap, nodeStatuses) {
  const svg = document.getElementById("tree-svg");
  svg.innerHTML = "";

  // Build variable positions: for each variable UUID, find nodes that
  // read/write it, and pick the midpoint of reader/writer positions.
  const varPos = new Map(); // varId -> {x, y}
  for (const [, node] of map) {
    const allVarIds = [...(node.reads || []), ...(node.writes || [])];
    for (const vId of allVarIds) {
      if (!varPos.has(vId)) {
        // Place var ellipse at bottom of SVG area, distributed
        varPos.set(vId, null); // placeholder
      }
    }
  }
  // Assign horizontal positions to variable ellipses below the tree
  const varIds = [...varPos.keys()];
  const varY = SVG_H - 30;
  varIds.forEach((id, i) => {
    const x = (SVG_W / (varIds.length + 1)) * (i + 1);
    varPos.set(id, { x, y: varY });
  });

  // Draw tree edges
  for (const [, node] of map) {
    if (!node.children) continue;
    for (const cid of node.children) {
      const child = map.get(cid);
      const isActive = nodeStatuses.get(node.id) !== undefined && nodeStatuses.get(cid) !== undefined;
      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.setAttribute("class", "bt-edge" + (isActive ? " active" : ""));
      line.setAttribute("x1", node.x);
      line.setAttribute("y1", node.y + (node.kind === "control" ? 24 : NODE_H / 2));
      line.setAttribute("x2", child.x);
      line.setAttribute("y2", child.y - (child.kind === "control" ? 24 : NODE_H / 2));
      svg.appendChild(line);
    }
  }

  // Draw dashed variable reference lines (reads = dashed blue, writes = dashed green)
  for (const [, node] of map) {
    for (const vId of (node.reads || [])) {
      const vp = varPos.get(vId);
      if (!vp) continue;
      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.setAttribute("class", "var-edge read");
      line.setAttribute("x1", node.x);
      line.setAttribute("y1", node.y + NODE_H / 2);
      line.setAttribute("x2", vp.x);
      line.setAttribute("y2", vp.y - 10);
      svg.appendChild(line);
    }
    for (const vId of (node.writes || [])) {
      const vp = varPos.get(vId);
      if (!vp) continue;
      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.setAttribute("class", "var-edge write");
      line.setAttribute("x1", vp.x);
      line.setAttribute("y1", vp.y - 10);
      line.setAttribute("x2", node.x);
      line.setAttribute("y2", node.y + NODE_H / 2);
      // arrowhead marker defined via marker-end
      line.setAttribute("marker-end", "url(#arrow-write)");
      svg.appendChild(line);
    }
  }

  // Draw tree nodes
  for (const [, node] of map) {
    const status = nodeStatuses.get(node.id) || "unvisited";
    const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
    g.setAttribute("class", `bt-node ${status}`);

    if (node.kind === "control") {
      const hw = NODE_W / 2;
      const hh = 24;
      const pts = [
        `${node.x},${node.y - hh}`,
        `${node.x + hw},${node.y}`,
        `${node.x},${node.y + hh}`,
        `${node.x - hw},${node.y}`,
      ].join(" ");
      const poly = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
      poly.setAttribute("points", pts);
      g.appendChild(poly);
    } else {
      const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      rect.setAttribute("x", node.x - NODE_W / 2);
      rect.setAttribute("y", node.y - NODE_H / 2);
      rect.setAttribute("width", NODE_W);
      rect.setAttribute("height", NODE_H);
      rect.setAttribute("rx", 6);
      g.appendChild(rect);
    }

    const lines = node.label.split("\n");
    const lineH = 13;
    const totalH = lines.length * lineH;
    lines.forEach((line, i) => {
      const t = document.createElementNS("http://www.w3.org/2000/svg", "text");
      t.setAttribute("text-anchor", "middle");
      t.setAttribute("dominant-baseline", "middle");
      t.setAttribute("x", node.x);
      t.setAttribute("y", node.y - totalH / 2 + lineH * i + lineH / 2);
      if (i > 0) t.setAttribute("class", "sub");
      t.textContent = line;
      g.appendChild(t);
    });

    svg.appendChild(g);
  }

  // Draw variable ellipses
  for (const [vId, pos] of varPos) {
    if (!pos) continue;
    const meta = varMap[vId];
    const name = meta ? meta.name : vId.slice(0, 8);

    const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
    g.setAttribute("class", "var-node");

    const ellipse = document.createElementNS("http://www.w3.org/2000/svg", "ellipse");
    ellipse.setAttribute("cx", pos.x);
    ellipse.setAttribute("cy", pos.y);
    ellipse.setAttribute("rx", 48);
    ellipse.setAttribute("ry", 16);
    g.appendChild(ellipse);

    const t = document.createElementNS("http://www.w3.org/2000/svg", "text");
    t.setAttribute("text-anchor", "middle");
    t.setAttribute("dominant-baseline", "middle");
    t.setAttribute("x", pos.x);
    t.setAttribute("y", pos.y);
    t.textContent = name;
    g.appendChild(t);

    svg.appendChild(g);
  }
}

function svgDefs() {
  const svg = document.getElementById("tree-svg");
  const defs = document.createElementNS("http://www.w3.org/2000/svg", "defs");
  const marker = document.createElementNS("http://www.w3.org/2000/svg", "marker");
  marker.setAttribute("id", "arrow-write");
  marker.setAttribute("markerWidth", "8");
  marker.setAttribute("markerHeight", "8");
  marker.setAttribute("refX", "6");
  marker.setAttribute("refY", "3");
  marker.setAttribute("orient", "auto");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", "M0,0 L0,6 L8,3 z");
  path.setAttribute("fill", "#4ade80");
  marker.appendChild(path);
  defs.appendChild(marker);
  svg.prepend(defs);
}

// ── Variable table ────────────────────────────────────────────────────────────

function renderVarTable(varMeta, values) {
  const tbody = document.getElementById("var-tbody");
  tbody.innerHTML = "";
  for (const [id, meta] of Object.entries(varMeta)) {
    const val = values[id];
    let display = "";
    if (val !== undefined) {
      if (val.f32 !== undefined) display = val.f32.toFixed(4);
      else if (val.str !== undefined) display = val.str;
      else display = JSON.stringify(val);
    } else if (meta.type === "f32") {
      display = meta.init.toFixed(4);
    } else {
      display = String(meta.init);
    }

    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="var-name-cell">${meta.name}</td>
      <td><input class="var-input" data-id="${id}" data-type="${meta.type}" value="${display}" /></td>
    `;
    tbody.appendChild(tr);
  }
}

function collectVarOverrides(varMeta) {
  const overrides = {};
  document.querySelectorAll(".var-input").forEach((inp) => {
    const id = inp.dataset.id;
    const type = inp.dataset.type;
    const raw = inp.value.trim();
    if (type === "f32") {
      const n = parseFloat(raw);
      overrides[id] = { f32: isNaN(n) ? 0.0 : n };
    } else {
      overrides[id] = { str: raw };
    }
  });
  return overrides;
}

// ── UI helpers ────────────────────────────────────────────────────────────────

function log(msg) {
  const el = document.getElementById("log");
  el.textContent += msg + "\n";
  el.scrollTop = el.scrollHeight;
}

function setStatus(status) {
  const el = document.getElementById("status-value");
  el.textContent = status || "—";
  el.className = status || "";
}

// ── Runner state ──────────────────────────────────────────────────────────────

let runner = null;
let modules = null; // { btHeaderYaml, btWasm, testHeaderYaml, testWasm }
let currentTree = null;
let nodeMap = null;
let nodeStatuses = new Map();
let tickCount = 0;
let currentVars = {};

async function loadModules(r) {
  // Two-step load: prepareModule compiles + instantiates asynchronously
  // (Chrome rejects both above 8 MB on the main thread), then
  // loadPreparedModule completes the load synchronously.
  const btHeader = JSON.stringify(jsyaml.load(modules.btHeaderYaml));
  const testHeader = JSON.stringify(jsyaml.load(modules.testHeaderYaml));
  await r.prepareModule(btHeader, new Uint8Array(modules.btWasm));
  r.loadPreparedModule(btHeader);
  await r.prepareModule(testHeader, new Uint8Array(modules.testWasm));
  r.loadPreparedModule(testHeader);
}

async function switchTree(treeKey) {
  runner = new BehaviorTreeRunner();
  await loadModules(runner);

  currentTree = TREES[treeKey];
  nodeMap = buildNodeMap(currentTree.nodes);
  layout(currentTree.nodes[0].id, nodeMap, SVG_W / 2, 50, SVG_W - 80);
  nodeStatuses = new Map();
  tickCount = 0;
  currentVars = {};

  document.getElementById("tick-count").textContent = 0;
  setStatus(null);

  // Initialise runner variables
  for (const [id, val] of Object.entries(currentTree.initial)) {
    runner.setVariable(id, JSON.stringify(val));
  }

  renderVarTable(currentTree.variables, currentVars);
  renderSVG(nodeMap, currentTree.variables, nodeStatuses);
  svgDefs();

  document.getElementById("tick-btn").disabled = false;
  document.getElementById("reset-btn").disabled = true;
  log(`\n--- switched to: ${currentTree.label} ---`);
}

function doTick() {
  // Apply any manual overrides from the table before ticking
  const overrides = collectVarOverrides(currentTree.variables);
  for (const [id, val] of Object.entries(overrides)) {
    runner.setVariable(id, JSON.stringify(val));
  }

  const nodesJson = JSON.stringify(toRustNodes(currentTree.nodes));
  try {
    const result = JSON.parse(runner.tick(nodesJson));
    tickCount++;
    document.getElementById("tick-count").textContent = tickCount;

    for (const entry of result.trace) {
      nodeStatuses.set(entry.nodeId, entry.status);
    }
    renderSVG(nodeMap, currentTree.variables, nodeStatuses);
    setStatus(result.status);

    currentVars = result.variables || {};
    renderVarTable(currentTree.variables, currentVars);

    const varSummary = Object.entries(currentTree.variables)
      .map(([id, m]) => {
        const v = currentVars[id];
        if (!v) return "";
        if (v.f32 !== undefined) return `${m.name}=${v.f32.toFixed(4)}`;
        if (v.str !== undefined) return `${m.name}="${v.str}"`;
        return "";
      })
      .filter(Boolean).join(", ");
    log(`tick ${tickCount}: ${result.status}${varSummary ? " | " + varSummary : ""}`);

    document.getElementById("reset-btn").disabled = false;
  } catch (e) {
    log(`ERROR: ${e}`);
    console.error(e);
  }
}

async function doReset() {
  runner = new BehaviorTreeRunner();
  await loadModules(runner);

  for (const [id, val] of Object.entries(currentTree.initial)) {
    runner.setVariable(id, JSON.stringify(val));
  }
  tickCount = 0;
  document.getElementById("tick-count").textContent = 0;
  nodeStatuses = new Map();
  currentVars = {};
  renderSVG(nodeMap, currentTree.variables, nodeStatuses);
  renderVarTable(currentTree.variables, currentVars);
  setStatus(null);
  document.getElementById("reset-btn").disabled = true;
  log("\n--- reset ---");
}

function toRustNodes(nodes) {
  return nodes.map(({ id, function: fn, children, arguments: args }) => {
    const n = { id, function: fn };
    if (children) n.children = children;
    if (args) n.arguments = args;
    return n;
  });
}

// ── Main ──────────────────────────────────────────────────────────────────────

async function main() {
  log("initializing arora-web wasm…");
  await init();

  log("fetching modules…");
  const [btHeaderYaml, btWasm, testHeaderYaml, testWasm] = await Promise.all([
    fetch("./modules/test-behavior-tree-nodes/module.yaml").then((r) => r.text()),
    fetch("./modules/test-behavior-tree-nodes/test_behavior_tree_nodes.wasm").then((r) => r.arrayBuffer()),
    fetch("./modules/test-rust-wasm/module.yaml").then((r) => r.text()),
    fetch("./modules/test-rust-wasm/test_rust_wasm.wasm").then((r) => r.arrayBuffer()),
  ]);
  modules = { btHeaderYaml, btWasm, testHeaderYaml, testWasm };
  log("modules loaded");

  // Populate tree dropdown
  const sel = document.getElementById("tree-select");
  for (const [key, tree] of Object.entries(TREES)) {
    const opt = document.createElement("option");
    opt.value = key;
    opt.textContent = tree.label;
    sel.appendChild(opt);
  }
  sel.addEventListener("change", () => switchTree(sel.value));

  document.getElementById("tick-btn").addEventListener("click", doTick);
  document.getElementById("reset-btn").addEventListener("click", doReset);

  await switchTree("classic");
}

main();
