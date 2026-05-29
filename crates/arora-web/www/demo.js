// demo.js — add+cos behavior-tree demo for the Arora browser engine.
//
// Each tick: add(x, 0.1) → x, then cos(x) → cos_result.
// Variables persist across ticks via BehaviorTreeRunner.setVariable / tick().

import init, { BehaviorTreeRunner } from "./pkg/arora_web.js";
import jsyaml from "https://cdn.jsdelivr.net/npm/js-yaml@4.1.0/+esm";

// ── UUIDs ─────────────────────────────────────────────────────────────────────

const VAR = {
  X:          "aaaa0001-0000-0000-0000-000000000000",
  COS_RESULT: "aaaa0002-0000-0000-0000-000000000000",
};

const FN = {
  SEQ:       "32246df6-ab5d-4f18-9221-23e28731de93",
  ADD:       "e4b0a2f3-6c7d-4e8f-9a0b-1c2d3e4f5a6b",
  COS:       "c13757cb-2311-4c93-abcc-cb12d6cbb859",
};

const PARAM = {
  ADD_A:     "a1b2c3d4-e5f6-4a8b-9c0d-e1f2a3b4c5d6",
  ADD_B:     "b2c3d4e5-f6a7-4b9c-8d1e-f2a3b4c5d6e7",
  COS_ANGLE: "6c2a157c-4235-47b0-bff3-1eeef3e5747d",
};

// ── Tree definition ───────────────────────────────────────────────────────────
//
//   Sequence
//   ├── add(x, 0.1) → x
//   └── cos(x) → cos_result

const TREE = [
  {
    id: "20000001-0000-0000-0000-000000000000",
    function: FN.SEQ,
    children: [
      "20000002-0000-0000-0000-000000000000",
      "20000003-0000-0000-0000-000000000000",
    ],
    label: "Sequence",
    kind: "control",
  },
  {
    id: "20000002-0000-0000-0000-000000000000",
    function: FN.ADD,
    arguments: {
      [PARAM.ADD_A]: { variable_id: VAR.X },
      [PARAM.ADD_B]: { value: { f32: 0.1 } },
    },
    return_binding: VAR.X,
    label: "add(x, 0.1) → x",
    kind: "action",
  },
  {
    id: "20000003-0000-0000-0000-000000000000",
    function: FN.COS,
    arguments: {
      [PARAM.COS_ANGLE]: { variable_id: VAR.X },
    },
    return_binding: VAR.COS_RESULT,
    label: "cos(x) → cos",
    kind: "action",
  },
];

function toRustNodes(tree) {
  return tree.map(({ id, function: fn, children, arguments: args, return_binding }) => {
    const n = { id, function: fn };
    if (children) n.children = children;
    if (args) n.arguments = args;
    if (return_binding) n.return_binding = return_binding;
    return n;
  });
}

// ── SVG layout ────────────────────────────────────────────────────────────────

const SVG_W = 640;
const SVG_H = 240;
const LEVEL_H = 100;
const NODE_W = 140;
const NODE_H = 38;

function buildNodeMap(tree) {
  const map = new Map();
  for (const n of tree) map.set(n.id, { ...n, x: 0, y: 0 });
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

function renderSVG(map, nodeStatuses) {
  const svg = document.getElementById("tree-svg");
  svg.innerHTML = "";

  for (const [, node] of map) {
    if (!node.children) continue;
    for (const cid of node.children) {
      const child = map.get(cid);
      const isActive = nodeStatuses.get(node.id) !== undefined && nodeStatuses.get(cid) !== undefined;
      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.setAttribute("class", "bt-edge" + (isActive ? " active" : ""));
      line.setAttribute("x1", node.x);
      line.setAttribute("y1", node.y + 24);
      line.setAttribute("x2", child.x);
      line.setAttribute("y2", child.y - 24);
      svg.appendChild(line);
    }
  }

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

function updateVars(vars) {
  const xVal = vars[VAR.X]?.f32 ?? 0;
  const cosVal = vars[VAR.COS_RESULT]?.f32 ?? 0;
  document.getElementById("var-x").textContent = xVal.toFixed(4);
  document.getElementById("var-cos").textContent = cosVal.toFixed(4);
}

// ── Main ──────────────────────────────────────────────────────────────────────

async function main() {
  log("initializing arora-web wasm…");
  await init();

  log("fetching modules…");
  const [btHeaderYaml, btWasm, testHeaderYaml, testWasm] = await Promise.all([
    fetch("./modules/behavior-tree-nodes/module.yaml").then((r) => r.text()),
    fetch("./modules/behavior-tree-nodes/behavior_tree_nodes.wasm").then((r) => r.arrayBuffer()),
    fetch("./modules/test-rust-wasm/module.yaml").then((r) => r.text()),
    fetch("./modules/test-rust-wasm/test_rust_wasm.wasm").then((r) => r.arrayBuffer()),
  ]);

  const runner = new BehaviorTreeRunner();
  runner.loadModule(JSON.stringify(jsyaml.load(btHeaderYaml)), new Uint8Array(btWasm));
  runner.loadModule(JSON.stringify(jsyaml.load(testHeaderYaml)), new Uint8Array(testWasm));
  log("modules loaded");

  // Initialize variables.
  runner.setVariable(VAR.X, JSON.stringify({ f32: 0.0 }));
  runner.setVariable(VAR.COS_RESULT, JSON.stringify({ f32: 1.0 }));

  const nodeMap = buildNodeMap(TREE);
  const rootId = TREE[0].id;
  layout(rootId, nodeMap, SVG_W / 2, 50, SVG_W - 60);
  const nodeStatuses = new Map();
  renderSVG(nodeMap, nodeStatuses);

  log("ready — click Tick");

  let tickCount = 0;
  const tickBtn = document.getElementById("tick-btn");
  const resetBtn = document.getElementById("reset-btn");
  tickBtn.disabled = false;

  const nodesJson = JSON.stringify(toRustNodes(TREE));

  tickBtn.addEventListener("click", () => {
    try {
      const result = JSON.parse(runner.tick(nodesJson));
      tickCount++;
      document.getElementById("tick-count").textContent = tickCount;

      for (const entry of result.trace) {
        nodeStatuses.set(entry.nodeId, entry.status);
      }
      renderSVG(nodeMap, nodeStatuses);
      setStatus(result.status);
      updateVars(result.variables);
      log(`tick ${tickCount}: x=${result.variables[VAR.X]?.f32?.toFixed(4)}, cos(x)=${result.variables[VAR.COS_RESULT]?.f32?.toFixed(4)}`);
      resetBtn.disabled = false;
    } catch (e) {
      log(`ERROR: ${e}`);
      console.error(e);
    }
  });

  resetBtn.addEventListener("click", () => {
    runner.setVariable(VAR.X, JSON.stringify({ f32: 0.0 }));
    runner.setVariable(VAR.COS_RESULT, JSON.stringify({ f32: 1.0 }));
    tickCount = 0;
    document.getElementById("tick-count").textContent = 0;
    nodeStatuses.clear();
    renderSVG(nodeMap, nodeStatuses);
    setStatus(null);
    updateVars({ [VAR.X]: { f32: 0.0 }, [VAR.COS_RESULT]: { f32: 1.0 } });
    log("\n--- reset ---");
    resetBtn.disabled = true;
  });
}

main();
