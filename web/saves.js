// Saved runs in IndexedDB. The host Worker stores the newest save at the
// start of every shift's Setup; the title screen offers "Continue run", and
// `?create&resume` hands the save to a new host Worker.
//
// Database "lastcall-saves", store "runs": one record under the key "latest":
// { json, savedAt }. `json` is a shared::save::RunSave.

const DB = 'lastcall-saves';
const STORE = 'runs';
const KEY = 'latest';

function open() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function run(mode, fn) {
  const db = await open();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    const req = fn(tx.objectStore(STORE));
    tx.oncomplete = () => resolve(req?.result);
    tx.onerror = () => reject(tx.error);
  });
}

/// Store the newest save (JSON text).
export const store = (json) => run('readwrite', (s) => s.put({ json, savedAt: Date.now() }, KEY));

/// The newest save as { json, savedAt }, or undefined.
export const latest = () => run('readonly', (s) => s.get(KEY));

/// Forget the saved run.
export const clear = () => run('readwrite', (s) => s.delete(KEY));
