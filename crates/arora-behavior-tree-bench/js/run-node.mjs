// node run-node.mjs module.wasm [workload.json ...]
// NODE_NATIVE=round.json also verifies exact per-tick native call/dispatch counts.
import { readFileSync, writeFileSync } from 'node:fs';
import { basename } from 'node:path';
import assert from 'node:assert/strict';
import { loadBenchWasm, workloadTable, scaleHeader, scaleRow } from './bench-wasm.mjs';
const [modulePath, ...paths] = process.argv.slice(2);
if (!modulePath) throw new Error('usage: run-node.mjs module.wasm [workload.json ...]');
const wasm = await loadBenchWasm(readFileSync(modulePath));
const samples = Number(process.env.BENCH_SAMPLES || 1), ticks = Number(process.env.BENCH_TICKS || wasm.defaults.ticks), warmUp = Number(process.env.BENCH_WARM_UP || wasm.defaults.warmUp);
const sizes = process.env.BENCH_SIZES ? process.env.BENCH_SIZES.split(',').map(Number) : wasm.defaults.sizes;
const forms = process.env.BENCH_FORMS?.split(',') || wasm.forms;
const native = process.env.NODE_NATIVE ? JSON.parse(readFileSync(process.env.NODE_NATIVE, 'utf8')) : null;
const output = { schema: 1, clock: wasm.clock, tests: {} };
let checked = 0;
function check(name, record) {
  for (const [form, r] of Object.entries(record.forms)) {
    assert.equal(r.differing, 0, `${name}/${form}: written values differ from detached`);
    if (native) {
      const expected = native.tests[name]?.record.forms[form];
      assert.ok(expected, `missing native ${name}/${form}`);
      // Hashes compare within each platform only; last bits of libm differ.
      for (const key of ['calls', 'dispatches']) assert.deepEqual(r[key], expected[key], `${name}/${form}: ${key}`);
      checked++;
    }
  }
  const hashes = Object.values(record.forms).map(r => r.written.join(','));
  assert.ok(hashes.every(h => h === hashes[0]), `${name}: form digests differ`);
}
for (const name of wasm.defaults.workloads) {
  const record = await wasm.loadBuiltin(name).measureAll({ samples, selected: forms });
  check(name, record); output.tests[name] = { kind: 'workload', size: 0, record };
  console.log(`${name}\n${workloadTable(record, forms)}`);
}
for (const path of paths) {
  const name = basename(path).replace(/\.json$/, '');
  if (output.tests[name]) throw new Error(`duplicate workload name: ${name}`);
  const record = await wasm.loadWorkload(readFileSync(path)).measureAll({ samples, selected: forms });
  check(name, record); output.tests[name] = { kind: 'workload', size: 0, record };
  console.log(workloadTable(record, forms));
}
console.log(scaleHeader);
for (const n of sizes) for (const scenario of wasm.scenarios) {
  const record = await wasm.loadScale(n, scenario, { ticks, warmUp }).measureAll({ samples, selected: forms });
  for (const form of forms) console.log(scaleRow(n, scenario, form, record.forms[form]));
  const name = wasm.scaleName(n, scenario); check(name, record); output.tests[name] = { kind: 'scale', size: n, record };
}
if (native) {
  assert.deepEqual(Object.keys(output.tests).sort(), Object.keys(native.tests).sort(), 'native and wasm test sets');
  console.error(`${checked} cells: native/WASI counts match at every tick; every form agrees within WASI`);
}
console.error(`clock: ${wasm.clock.stepUs.toFixed(3)} µs step, crossOriginIsolated=${wasm.clock.crossOriginIsolated}, minimum sample=${wasm.clock.minimumSampleUs.toFixed(3)} µs`);
if (process.env.NODE_TIMES) writeFileSync(process.env.NODE_TIMES, JSON.stringify(output));
