// The RNG audit log in IndexedDB (plan section 3.2). The host Worker appends
// JSONL chunks for its run; the host page exports a run as one JSONL file,
// which `tools replay` (or `host_audit_verify` in the browser) re-derives.
//
// Database "lastcall-audit", store "chunks": { run, text }, keys in write
// order. Only the newest KEEP_RUNS runs are kept.

const DB = 'lastcall-audit';
const STORE = 'chunks';
const KEEP_RUNS = 3;

function open() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => {
      const store = req.result.createObjectStore(STORE, { autoIncrement: true });
      store.createIndex('run', 'run');
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

let dbPromise = null;
const db = () => (dbPromise ??= open());

function done(tx) {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

/// A new run id: sortable by start time.
export function newRun() {
  return `${Date.now().toString().padStart(15, '0')}-${Math.random().toString(36).slice(2, 8)}`;
}

/// Append a JSONL chunk to a run.
export async function append(run, text) {
  const d = await db();
  const tx = d.transaction(STORE, 'readwrite');
  tx.objectStore(STORE).add({ run, text });
  await done(tx);
}

/// Run ids in the log, oldest first.
export async function runs() {
  const d = await db();
  const tx = d.transaction(STORE, 'readonly');
  const req = tx.objectStore(STORE).index('run').openKeyCursor(null, 'nextunique');
  const out = [];
  await new Promise((resolve, reject) => {
    req.onsuccess = () => {
      const c = req.result;
      if (!c) return resolve();
      out.push(c.key);
      c.continue();
    };
    req.onerror = () => reject(req.error);
  });
  return out;
}

/// Drop all but the newest runs.
export async function prune(keep = KEEP_RUNS) {
  const all = await runs();
  const old = all.slice(0, Math.max(0, all.length - keep));
  if (!old.length) return;
  const d = await db();
  const tx = d.transaction(STORE, 'readwrite');
  const index = tx.objectStore(STORE).index('run');
  for (const run of old) {
    const req = index.openCursor(IDBKeyRange.only(run));
    req.onsuccess = () => {
      const c = req.result;
      if (c) {
        c.delete();
        c.continue();
      }
    };
  }
  await done(tx);
}

/// A run's whole log as JSONL text (the newest run when none is given).
export async function exportRun(run) {
  run ??= (await runs()).at(-1);
  if (!run) return '';
  const d = await db();
  const tx = d.transaction(STORE, 'readonly');
  const req = tx.objectStore(STORE).index('run').getAll(IDBKeyRange.only(run));
  const rows = await new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  return rows.map((r) => r.text).join('');
}

/// Save a run's log as a file (the debug export).
export async function download(run) {
  const text = await exportRun(run);
  const url = URL.createObjectURL(new Blob([text], { type: 'application/x-ndjson' }));
  const a = Object.assign(document.createElement('a'), { href: url, download: `lastcall-rng-${run ?? 'latest'}.jsonl` });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
