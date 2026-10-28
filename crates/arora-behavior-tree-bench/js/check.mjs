// node check.mjs module.wasm synthetic.json
// Exercises the C ABI, interleaving, errors and a deliberately coarsened clock.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { loadBenchWasm, clockStep, workloadTable, scaleRow } from './bench-wasm.mjs';
const [modulePath, workloadPath] = process.argv.slice(2);
const bytes = readFileSync(modulePath), text = readFileSync(workloadPath, 'utf8');
const wasm = await loadBenchWasm(bytes), x = wasm.exports;
const selected = wasm.forms.filter(f => f !== 'test-wrong');
assert.deepEqual(wasm.defaults.workloads, ['synthetic', 'conversation']);
assert.equal(x.bench_builtin_load(wasm.defaults.workloads.length), 1);
assert.throws(() => wasm.loadBuiltin('missing'), /unknown built-in/);

let readings = 0;
assert.equal(clockStep(() => Math.floor(readings++ / 10)), 1);
assert.throws(() => clockStep(() => 0), /did not advance/);
assert.equal(x.bench_workload_begin(0), 1);
assert.equal(x.bench_workload_sample(), 0);
assert.equal(x.bench_workload_finish(), 1);
assert.equal(x.bench_measure(0, 0, 0, 0, 10, 1), 1);
assert.equal(x.bench_measure(10, 0, 0, 0, 10, 0), 1);
assert.equal(x.bench_measure(10, 0, 0, 0, 10, 2), 1);
// A warm-up plus ticks past u32 is invalid input, not a trap.
assert.equal(x.bench_measure(10, 0, 0, 0xffffffff, 1, 1), 1);
assert.equal(x.bench_scale_load(10, 0, 0xffffffff, 1), 1);
assert.equal(x.bench_minimum_sample_us(-1), 1);
assert.equal(x.bench_minimum_sample_us(NaN), 1);
assert.throws(() => wasm.loadWorkload('{'), /rejected/);
const badRoot = JSON.parse(text); badRoot.text = 'missing_function()';
assert.throws(() => wasm.loadWorkload(JSON.stringify(badRoot)), /rejected/);
const badLowering = JSON.parse(text);
badLowering.text = 'running\n\ng: {S} = hold(t: {arora/time}, duration: 0, value: 1, out: {.o}, state: {k}[/0])';
// Both direct lowering and an omitted detached reference return diagnostics.
for (const form of selected) {
  const session = wasm.loadWorkload(JSON.stringify(badLowering));
  session.begin(form);
  if (form === 'detached') {
    assert.equal(x.bench_workload_sample(), 0, 'lowering returns an error count without trapping');
  } else {
    assert.equal(x.bench_workload_sample(), 1);
    assert.equal(x.bench_workload_finish(), 1, 'reference lowering returns an error status without trapping');
  }
  const diagnostic = JSON.parse(new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.bench_result(), x.bench_result_len())));
  assert.match(diagnostic.error, /^detached:.*inconsistent behavior tree/);
  assert.throws(() => wasm.loadWorkload(JSON.stringify(badLowering)).measure(form, { samples: 1 }), /detached:.*inconsistent behavior tree/);
}
// A tick failure returns its diagnostic the same way, without trapping.
const badTick = JSON.parse(text);
badTick.text = 'hold(t: {arora/time}, duration: 0, value: 1, out: {target}, state: f64[])';
for (const form of selected) {
  assert.throws(() => wasm.loadWorkload(JSON.stringify(badTick)).measure(form, { samples: 1 }), /at tick 0:.*a saved state holds 3 numbers, not 0/);
}
const one = wasm.loadWorkload(text).measure('attached-naive', { samples: 1 });
assert.equal(one.forms['attached-naive'].differing, 0, 'detached reference when not selected');
for (const name of wasm.defaults.workloads) {
  const record = await wasm.loadBuiltin(name).measureAll({ samples: 1, selected });
  for (const form of selected) assert.equal(record.forms[form].differing, 0, `${name}/${form}`);
}
const baseline = await wasm.loadWorkload(text).measureAll({ samples: 3, selected });
for (const form of selected) assert.equal(baseline.forms[form].differing, 0);
assert.equal(x.bench_workload_row(7), 0);
assert.equal(x.bench_workload_series_len(), Math.ceil((baseline.ticks - baseline.warmUp) / 15) * 3);
assert.ok(Number.isNaN(x.bench_workload_row(10000)));
// This explicit check build supplies a wrong form and a virtual work clock;
// neither fixture is included in the distributable module.
assert.equal(typeof x.bench_check_virtual, 'function', 'build the check module with --features bench-check');
for (const selected of [['test-wrong'], ['detached', 'test-wrong']]) {
  const wrong = await wasm.loadWorkload(text).measureAll({ samples: 1, selected });
  assert.ok(wrong.forms['test-wrong'].differing > 0, 'detached reference must detect wrong form even when not selected');
}
for (const [step, samples] of [[0, 3], [100, 1], [100, 3]]) {
  let sum = 0;
  for (let seed = 1; seed <= 128; seed++) {
    x.bench_check_virtual(step, seed * 7919, samples);
    const record = JSON.parse(new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.bench_result(), x.bench_result_len())));
    sum += record.forms.detached.summary.mean;
  }
  const mean = sum / 128;
  console.log(`virtual clock: step=${step} µs; samples=${samples}; mean=${mean.toFixed(6)} µs; truth=1.000000 µs`);
  assert.ok(Math.abs(mean - 1) < 0.005, `coarse-clock mean ${mean}`);
}

const coarse = await loadBenchWasm(bytes, { now: () => Math.floor(performance.now()) });
const repeated = await coarse.loadWorkload(text).measureAll({ samples: 1, selected });
assert.equal(repeated.clock.stepUs, 1000);
for (const form of selected) {
  const r = repeated.forms[form], base = baseline.forms[form];
  assert.equal(r.differing, 0);
  assert.deepEqual(r.calls, base.calls);
  assert.deepEqual(r.dispatches, base.dispatches);
  assert.deepEqual(r.written, base.written);
  const span = r.replayUs[0].reduce((a, b) => a + b, 0);
  assert.equal(r.timingMode[0], 'replay');
  assert.deepEqual(repeated.clock.modes[form], r.timingMode);
  assert.equal(r.replayUs[0].length, r.repetitions[0]);
  for (const key of ['p50', 'p95', 'p99', 'max']) assert.equal(r.summary[key], null);
  assert.ok(r.summary.phases.every(x => x === null));
  assert.ok(r.summary.bins.every(bin => bin[0] === null));
  console.log(`${form}: clock step ${repeated.clock.stepUs} µs; ${r.repetitions[0]} replays; timed span ${span.toFixed(0)} µs; differing=${r.differing}`);
}
assert.ok(workloadTable(repeated).includes('unresolved'));
assert.ok(scaleRow(10, 'settled', 'detached', repeated.forms.detached).includes('unresolved'));
for (const scenario of wasm.scenarios) {
  const r = await wasm.loadScale(10, scenario, { warmUp: 16, ticks: 144 }).measureAll({ samples: 3, selected });
  for (const f of selected) assert.equal(r.forms[f].differing, 0);
}
// Fixed scale row ABI, alongside the full JSON record interface.
assert.equal(x.bench_measure(10, 0, 0, 16, 144, 1), 0);
const data = new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.bench_result(), x.bench_result_len()));
const row = JSON.parse(data).forms[wasm.forms[0]];
assert.equal(x.bench_row(3), row.summary.calls);
assert.equal(x.bench_row(4), row.summary.dispatches);
assert.ok(Number.isNaN(x.bench_row(10000)));
console.log('ok: C ABI, interleaved samples, registry, errors and coarsened clock');
