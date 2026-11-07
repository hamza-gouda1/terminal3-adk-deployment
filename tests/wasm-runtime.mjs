// Copyright (c) 2026 Hamza Gouda
// Exercise the actual wit-bindgen 0.24 core-WASM ABI with a deterministic host.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const bytes = await readFile(process.argv[2] ?? 'target/wasm32-wasip1/release/agent_budget_guard.wasm');
const module = await WebAssembly.compile(bytes);
const store = new Map();
let instance;
let now = 100n;
let clockError = false;
let failWrite = false;
const text = new TextDecoder();
const encoder = new TextEncoder();
const memory = () => new DataView(instance.exports.memory.buffer);
const read = (ptr, len) => text.decode(new Uint8Array(instance.exports.memory.buffer, ptr, len));
function alloc(str) {
  const bytes = encoder.encode(str);
  const ptr = instance.exports.cabi_realloc(0, 0, 1, bytes.length);
  new Uint8Array(instance.exports.memory.buffer, ptr, bytes.length).set(bytes);
  return [ptr, bytes.length];
}
const imports = {
  'terminal3:agent-guard/logging': { 'log-info': () => {}, 'log-warn': () => {}, 'log-error': () => {} },
  'terminal3:agent-guard/kv-store': {
    get(k, kl, out) {
      const value = store.get(read(k, kl));
      if (value === undefined) { memory().setUint8(out, 0); return; }
      const [ptr, len] = alloc(value);
      memory().setUint8(out, 1);
      memory().setUint32(out + 4, ptr, true);
      memory().setUint32(out + 8, len, true);
    },
    set(k, kl, v, vl, out) {
      if (failWrite) {
        const [ptr, len] = alloc('host write failed');
        memory().setUint8(out, 1);
        memory().setUint32(out + 4, ptr, true);
        memory().setUint32(out + 8, len, true);
      } else {
        store.set(read(k, kl), read(v, vl));
        memory().setUint8(out, 0);
      }
    },
  },
  wasi_snapshot_preview1: {
    clock_time_get(id, precision, ptr) {
      assert.equal(id, 0);
      if (clockError) return 29; // WASI IO error
      memory().setBigUint64(ptr, now * 1_000_000_000n, true);
      return 0;
    },
    environ_sizes_get(count, size) { memory().setUint32(count, 0, true); memory().setUint32(size, 0, true); return 0; },
    environ_get() { return 0; },
    fd_write() { throw new Error('unexpected WASI write'); },
    proc_exit(code) { throw new Error(`unexpected WASI exit ${code}`); },
  },
};
instance = await WebAssembly.instantiate(module, imports);
function setBudget() {
  const [ap, al] = alloc('did:agent');
  const [cp, cl] = alloc('did:owner');
  const ret = instance.exports['set-budget'](ap, al, 10n, 20n, 10n, cp, cl);
  const tag = memory().getUint8(ret);
  const ok = tag === 0 && memory().getUint8(ret + 4) === 1;
  const message = tag ? read(memory().getUint32(ret + 4, true), memory().getUint32(ret + 8, true)) : null;
  instance.exports['cabi_post_set-budget'](ret);
  return { ok, message };
}
function consume(caller, tokens, usd) {
  const [ap, al] = alloc('did:agent');
  const [cp, cl] = alloc(caller);
  const [mp, ml] = alloc('test');
  const ret = instance.exports['consume-budget'](ap, al, cp, cl, BigInt(tokens), BigInt(usd), mp, ml);
  const result = { approved: !!memory().getUint8(ret), tokens: memory().getBigUint64(ret + 8, true), usd: memory().getBigUint64(ret + 16, true) };
  instance.exports['cabi_post_consume-budget'](ret);
  return result;
}
function status() {
  const [p, l] = alloc('did:agent');
  const ret = instance.exports['get-budget'](p, l);
  assert.equal(memory().getUint8(ret), 1);
  const result = { consumed: memory().getBigUint64(ret + 32, true), start: memory().getBigUint64(ret + 64, true), end: memory().getBigUint64(ret + 72, true) };
  instance.exports['cabi_post_get-budget'](ret);
  return result;
}
const clockEnabled = WebAssembly.Module.imports(module).some(item => item.name === 'clock_time_get');
assert.equal(clockEnabled, !process.argv.includes('--default'));
if (!clockEnabled) {
  assert.match(setBudget().message, /clock unavailable/i);
  store.set('agent_guard:budget:did:agent', JSON.stringify({
    agent_did: 'did:agent', max_tokens_per_epoch: 10, max_usd_micro_per_epoch: 20,
    epoch_duration_sec: 10, authorized_caller: 'did:owner', tokens_consumed: 0,
    usd_micro_consumed: 0, epoch_start_timestamp: 100, epoch_end_timestamp: 110,
    is_active: true, audit_counter: 0,
  }));
  assert.equal(consume('did:owner', 1, 1).approved, false);
  assert.deepEqual(status(), { consumed: 0n, start: 100n, end: 110n });
  console.log('Default compiled WASM fails closed without a trusted clock');
} else {
  assert.equal(setBudget().ok, true);
  assert.equal(consume('intruder', 1, 1).approved, false);
  assert.deepEqual(consume('did:owner', 10, 20), { approved: true, tokens: 0n, usd: 0n });
  assert.equal(consume('did:owner', 1, 0).approved, false);
  now = 110n;
  assert.equal(consume('intruder', 1, 1).approved, false);
  assert.deepEqual(status(), { consumed: 10n, start: 100n, end: 110n });
  assert.deepEqual(consume('did:owner', 2, 3), { approved: true, tokens: 8n, usd: 17n });
  assert.deepEqual(status(), { consumed: 2n, start: 110n, end: 120n });
  now = 145n;
  clockError = true;
  assert.equal(consume('did:owner', 1, 1).approved, false);
  assert.equal(setBudget().ok, false);
  clockError = false;
  failWrite = true;
  assert.equal(consume('did:owner', 1, 1).approved, false);
  assert.deepEqual(status(), { consumed: 2n, start: 110n, end: 120n });
  failWrite = false;
  assert.equal(consume('did:owner', 1, 1).approved, true);
  assert.deepEqual(status(), { consumed: 1n, start: 140n, end: 150n });
  console.log('Compiled WASM runtime behavior passed with controlled WASI clock and in-memory KV');
}
