// node check.mjs module.wasm synthetic.json
// Exercises the C ABI, interleaving, errors and a deliberately coarsened clock.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { loadBenchWasm, clockStep } from './bench-wasm.mjs';
const [modulePath, workloadPath] = process.argv.slice(2);
const bytes = readFileSync(modulePath), text = readFileSync(workloadPath, 'utf8');
const wasm = await loadBenchWasm(bytes), x = wasm.exports;
let readings = 0;
assert.equal(clockStep(() => Math.floor(readings++ / 10)), 1);
assert.throws(() => clockStep(() => 0), /did not advance/);
assert.equal(x.bench_workload_begin(0), 1);
assert.equal(x.bench_workload_sample(), 0);
assert.equal(x.bench_workload_finish(), 1);
assert.equal(x.bench_measure(0, 0, 0, 0, 10, 1), 1);
assert.equal(x.bench_measure(10, 0, 0, 0, 10, 0), 1);
assert.equal(x.bench_measure(10, 0, 0, 0, 10, 2), 1);
assert.equal(x.bench_minimum_sample_us(-1), 1);
assert.equal(x.bench_minimum_sample_us(NaN), 1);
assert.throws(() => wasm.loadWorkload('{'), /rejected/);
const badRoot = JSON.parse(text); badRoot.tree = { k: 'calls', calls: [] };
assert.throws(() => wasm.loadWorkload(JSON.stringify(badRoot)), /rejected/);
const one = wasm.loadWorkload(text).measure('attached-naive', { samples: 1 });
assert.equal(one.forms['attached-naive'].differing, 0, 'detached reference when not selected');
const baseline = await wasm.loadWorkload(text).measureAll({ samples: 3 });
for (const form of wasm.forms) assert.equal(baseline.forms[form].differing, 0);
assert.equal(x.bench_workload_row(7), 0);
assert.equal(x.bench_workload_series_len(), Math.ceil((baseline.ticks - baseline.warmUp) / 15) * 3);
assert.ok(Number.isNaN(x.bench_workload_row(10000)));
const coarse = await loadBenchWasm(bytes, { now: () => Math.floor(performance.now()) });
const repeated = await coarse.loadWorkload(text).measureAll({ samples: 1 });
assert.equal(repeated.clock.stepUs, 1000);
for (const form of coarse.forms) {
  const r = repeated.forms[form], base = baseline.forms[form];
  assert.equal(r.differing, 0);
  assert.deepEqual(r.calls, base.calls);
  assert.deepEqual(r.dispatches, base.dispatches);
  assert.deepEqual(r.written, base.written);
  const span = r.times[0].slice(repeated.warmUp).reduce((a, b) => a + b, 0) * r.repetitions[0];
  assert.ok(span >= repeated.clock.minimumSampleUs - 1e-6, `${form}: at least 50 clock steps`);
  assert.equal(r.replayTimes[0].length, r.repetitions[0]);
  console.log(`${form}: clock step ${repeated.clock.stepUs} µs; ${r.repetitions[0]} replays; timed span ${span.toFixed(0)} µs; differing=${r.differing}`);
}
for (const scenario of wasm.scenarios) {
  const r = await wasm.loadScale(10, scenario, { warmUp: 16, ticks: 144 }).measureAll({ samples: 3 });
  for (const f of wasm.forms) assert.equal(r.forms[f].differing, 0);
}
// Fixed scale row ABI, alongside the full JSON record interface.
assert.equal(x.bench_measure(10, 0, 0, 16, 144, 1), 0);
const data = new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.bench_result(), x.bench_result_len()));
const row = JSON.parse(data).forms[wasm.forms[0]];
assert.equal(x.bench_row(3), row.summary.calls);
assert.equal(x.bench_row(4), row.summary.dispatches);
assert.ok(Number.isNaN(x.bench_row(10000)));
console.log('ok: C ABI, interleaved samples, registry, errors and coarsened clock');
