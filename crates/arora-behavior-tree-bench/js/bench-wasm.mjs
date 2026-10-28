// WASI preview 1 loader, shared by Node and browsers. No Arora host is required.
export function clockStep(now = () => performance.now()) {
  let previous = now(), step = Infinity;
  for (let transitions = 0, reads = 0; transitions < 32 && reads < 2000000; reads++) {
    const current = now(), delta = current - previous;
    if (delta > 0) { step = Math.min(step, delta); transitions++; previous = current; }
  }
  if (!Number.isFinite(step)) throw new Error('monotonic clock did not advance');
  return step;
}

export async function loadBenchWasm(bytes, { now = () => performance.now(), random = globalThis.crypto } = {}) {
  if (!random && typeof process !== 'undefined') random = (await import('node:crypto')).webcrypto;
  const stepMs = clockStep(now);
  const clock = { stepUs: stepMs * 1000, crossOriginIsolated: globalThis.crossOriginIsolated === true, minimumSampleUs: stepMs * 1000 * 50 };
  let memory, stderr = '';
  const view = () => new DataView(memory.buffer);
  const decoder = new TextDecoder();
  const wasi = {
    random_get(ptr, len) {
      for (let at = 0; at < len; at += 65536) random.getRandomValues(new Uint8Array(memory.buffer, ptr + at, Math.min(65536, len - at)));
      return 0;
    },
    environ_sizes_get(count, size) { view().setUint32(count, 0, true); view().setUint32(size, 0, true); return 0; },
    environ_get() { return 0; },
    clock_time_get(id, _precision, out) {
      const ms = id === 0 ? performance.timeOrigin + now() : now();
      view().setBigUint64(out, BigInt(Math.round(ms * 1e6)), true); return 0;
    },
    fd_write(_fd, iovs, count, written) {
      let total = 0;
      for (let i = 0; i < count; i++) {
        const ptr = view().getUint32(iovs + 8 * i, true), len = view().getUint32(iovs + 8 * i + 4, true);
        stderr += decoder.decode(new Uint8Array(memory.buffer, ptr, len)); total += len;
      }
      view().setUint32(written, total, true); return 0;
    },
    proc_exit(code) { throw new Error(`benchmark exited with ${code}`); },
    sched_yield() { return 0; },
  };
  const { instance } = await WebAssembly.instantiate(bytes, { wasi_snapshot_preview1: wasi });
  const x = instance.exports; memory = x.memory;
  // A cdylib may require reactor initialization on toolchains that export it.
  if (x._initialize) x._initialize();
  const text = (ptr, len) => decoder.decode(new Uint8Array(memory.buffer, ptr, len));
  const call = (f) => { stderr = ''; try { return f(); } catch (e) { throw new Error(stderr.trim() || e.message); } };
  const check = (f) => { if (call(f) !== 0) throw new Error('benchmark rejected arguments or workload'); };
  const result = () => {
    const record = JSON.parse(text(x.bench_result(), x.bench_result_len()));
    return { ...record, clock: { ...clock, modes: record.clock.modes } };
  };
  const [scenarios, forms] = text(x.bench_names(), x.bench_names_len()).split(';').map(s => s.split(','));
  const defaults = JSON.parse(text(x.bench_defaults(), x.bench_defaults_len()));
  check(() => x.bench_clock_step_us(clock.stepUs));
  check(() => x.bench_minimum_sample_us(clock.minimumSampleUs));
  const formIndex = form => { const f = forms.indexOf(form); if (f < 0) throw new Error(`unknown form: ${form}`); return f; };
  function loadWorkload(source) {
    const data = typeof source === 'string' ? new TextEncoder().encode(source) : source;
    const ptr = x.bench_alloc(data.length);
    new Uint8Array(memory.buffer, ptr, data.length).set(data);
    try { check(() => x.bench_workload_load(ptr, data.length)); } finally { x.bench_free(ptr, data.length); }
    return session();
  }
  function session() {
    const [, phaseNames, meta] = text(x.bench_workload_names(), x.bench_workload_names_len()).split(';');
    const [calls, ticks, warmUp, dt] = meta.split(',').map(Number);
    const workload = {
      forms, phases: phaseNames.split(','), calls, ticks, warmUp, dt,
      begin(form) { check(() => x.bench_workload_begin(formIndex(form))); },
      select(form) { check(() => x.bench_workload_select(formIndex(form))); },
      sample() { const n = call(() => x.bench_workload_sample()); if (!n) throw new Error('begin a form first'); return n; },
      finish() { check(() => x.bench_workload_finish()); return result(); },
      measure(form, { samples = 7 } = {}) {
        if (!Number.isInteger(samples) || samples <= 0 || samples % 2 === 0) throw new Error('positive odd samples required');
        workload.begin(form); for (let s = 0; s < samples; s++) workload.sample(); return workload.finish();
      },
      // Forms are interleaved inside each sample, as in the native harness.
      async measureAll({ samples = 7, selected = forms, yieldBetween = async () => {} } = {}) {
        if (!Number.isInteger(samples) || samples <= 0 || samples % 2 === 0 || !selected.length || new Set(selected).size !== selected.length) throw new Error('positive odd samples and unique forms required');
        for (const f of selected) workload.begin(f);
        for (let s = 0; s < samples; s++) for (const f of selected) { workload.select(f); workload.sample(); await yieldBetween(); }
        let combined;
        for (const f of selected) { workload.select(f); const r = workload.finish(); if (!combined) combined = r; else { Object.assign(combined.forms, r.forms); Object.assign(combined.clock.modes, r.clock.modes); } }
        return combined;
      },
    };
    return workload;
  }
  return {
    forms, scenarios, clock, defaults, loadWorkload,
    scaleName(n, scenario) {
      check(() => x.bench_scale_name(n, scenarios.indexOf(scenario)));
      return JSON.parse(text(x.bench_result(), x.bench_result_len()));
    },
    loadScale(n, scenario, { warmUp = defaults.warmUp, ticks = defaults.ticks } = {}) {
      const s = scenarios.indexOf(scenario);
      if (s < 0 || ![n, ticks].every(v => Number.isInteger(v) && v > 0) || !Number.isInteger(warmUp) || warmUp < 0) throw new Error('invalid scale cell');
      check(() => x.bench_scale_load(n, s, warmUp, ticks)); return session();
    },
    measure(n, scenario, form, { warmUp = defaults.warmUp, ticks = defaults.ticks, samples = 7 } = {}) {
      const s = scenarios.indexOf(scenario), f = formIndex(form);
      if (s < 0 || ![n, ticks, samples].every(v => Number.isInteger(v) && v > 0) || !Number.isInteger(warmUp) || warmUp < 0 || samples % 2 === 0) throw new Error('invalid scale cell');
      check(() => x.bench_measure(n, s, f, warmUp, ticks, samples)); return result();
    },
    // For tools that need the fixed row/series C ABI rather than JSON.
    exports: x,
  };
}

export function workloadTable(record, forms = Object.keys(record.forms)) {
  const phases = record.phases;
  const lines = [`${record.ticks} ticks at ${(1 / record.dt).toFixed(0)} Hz, warm-up=${record.warmUp}; µs per tick includes host writes`,
    '| form | mean us | p50 | p95 | p99 | max | calls | dispatches |' + phases.map(p => ` ${p} us |`).join('') + ' ticks differing |',
    '|---|' + '---:|'.repeat(8 + phases.length)];
  for (const form of forms) { const r = record.forms[form], s = r.summary; lines.push(`| ${form} | ` + [s.mean, s.p50, s.p95, s.p99, s.max, s.calls, s.dispatches, ...s.phases].map(x => x === null ? 'unresolved' : x.toFixed(3)).join(' | ') + ` | ${r.differing} |`); }
  return lines.join('\n');
}
export const scaleHeader = '| N | scenario | form | mean us | p50 | p95 | p99 | max | calls | dispatches | ticks differing |\n|---:|---|---|---:|---:|---:|---:|---:|---:|---:|---:|';
export function scaleRow(n, scenario, form, r) {
  const s = r.summary;
  return `| ${n} | ${scenario} | ${form} | ` + [s.mean, s.p50, s.p95, s.p99, s.max, s.calls, s.dispatches].map(x => x === null ? 'unresolved' : x.toFixed(3)).join(' | ') + ` | ${r.differing} |`;
}
