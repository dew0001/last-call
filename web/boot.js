// Boots LAST CALL: picks the renderer, then runs one of three modes.
//
//   /               title screen with the lobby (Open the bar / join by code)
//   /?create        host a room: start the host Worker, show the join link
//   /?room=CODE     join a room over WebRTC
//   /?hostonly      test page: only the host Worker, no renderer
//
// Bevy's WebGPU build cannot fall back to WebGL2, so there are two client
// bundles. `?gpu=webgl2` or `?gpu=webgpu` forces one; otherwise WebGPU is used
// when the browser gives us an adapter.
import { hostRoom, joinRoom, newRoomCode, parseRoomCode } from './net.js';
import { startVoice } from './voice.js';

const params = new URLSearchParams(location.search);
const lobby = document.getElementById('lobby');
const banner = document.getElementById('banner');

function show(el, text) {
  if (!el) return;
  el.hidden = false;
  if (text !== undefined) el.textContent = text;
}

// ---------- identity ----------

function storageGet(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storageSet(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

/** Player UUID from localStorage, so a refreshed tab gets its old player back. */
function playerUuid() {
  // `?player=` gives test tabs in one browser profile separate identities.
  const forced = params.get('player');
  if (forced) return forced;
  let id = storageGet('lastcall.uuid');
  if (!id) {
    id = crypto.randomUUID();
    storageSet('lastcall.uuid', id);
  }
  return id;
}

function displayName() {
  return params.get('name') ?? storageGet('lastcall.name') ?? 'Patron';
}

// ---------- renderer selection ----------

async function webgpuAvailable() {
  try {
    if (!('gpu' in navigator)) return false;
    // Some browsers never settle this promise; treat 2 s of silence as "no".
    const timeout = new Promise((resolve) => setTimeout(() => resolve(null), 2000));
    return (await Promise.race([navigator.gpu.requestAdapter(), timeout])) !== null;
  } catch {
    return false;
  }
}

const FALLBACK_KEY = 'lastcall.webgpuFailed';

function webgpuFailedBefore() {
  try {
    return sessionStorage.getItem(FALLBACK_KEY) === '1';
  } catch {
    return false;
  }
}

async function pickVariant() {
  const forced = params.get('gpu');
  if (forced === 'webgl2' || forced === 'webgpu') return forced;
  if (webgpuFailedBefore()) return 'webgl2';
  return (await webgpuAvailable()) ? 'webgpu' : 'webgl2';
}

// Some browsers hand out a WebGPU adapter whose device dies right away (for
// example headless Chromium on SwiftShader). When the auto-picked WebGPU build
// stops drawing early, remember that for this tab and reload on WebGL2.
function watchWebgpu() {
  const started = performance.now();
  const timer = setInterval(() => {
    const frames = window.__lastCall?.frames ?? 0;
    if (frames > 30) {
      clearInterval(timer);
      return;
    }
    if (performance.now() - started > 20000) {
      clearInterval(timer);
      try {
        sessionStorage.setItem(FALLBACK_KEY, '1');
      } catch {
        location.search = '?gpu=webgl2';
        return;
      }
      location.reload();
    }
  }, 500);
}

// A browser can fail to create its first GL or GPU context right after it
// starts (seen in WebKit). If the renderer fails before the first frame,
// reload once. A second failure is shown to the player.
//
// `window.__lastCallError` is an accessor: the first error is kept (a Rust
// panic message is more useful than the crash that follows it), and the
// retry decision happens in the same moment the error is written, so a test
// never sees an error without also seeing the retry flag.
const RETRY_KEY = 'lastcall.startRetries';
const RENDERER_START_ERRORS = [
  'Unable to find a GPU',
  'Failed to create wgpu surface',
  'getContext() returned null',
  'SuperDecompressionError',
];

function startRetries() {
  try {
    return Number(sessionStorage.getItem(RETRY_KEY) ?? 0);
  } catch {
    return 99;
  }
}

let firstError;
function onClientError(err) {
  const frames = window.__lastCall?.frames ?? 0;
  const retries = startRetries();
  if (frames === 0 && retries < 1 && RENDERER_START_ERRORS.some((m) => err.includes(m))) {
    try {
      sessionStorage.setItem(RETRY_KEY, String(retries + 1));
    } catch {}
    window.__lastCallRetrying = true;
    setTimeout(() => location.reload(), 50);
    return;
  }
  show(banner, frames === 0 ? 'Your browser could not start the 3D renderer. Try another browser.' : `Error: ${err}`);
}
Object.defineProperty(window, '__lastCallError', {
  configurable: true,
  get: () => firstError,
  set(value) {
    if (firstError !== undefined) return;
    firstError = String(value);
    onClientError(firstError);
  },
});

// Clear the retry counter once the renderer has drawn.
function watchStartFailure() {
  const timer = setInterval(() => {
    if ((window.__lastCall?.frames ?? 0) > 0) {
      clearInterval(timer);
      try {
        sessionStorage.removeItem(RETRY_KEY);
      } catch {}
    }
  }, 500);
}

/** Load the client module for the chosen renderer. Returns its exports. */
async function loadClient() {
  const variant = await pickVariant();
  window.__lastCallVariant = variant;
  if (variant === 'webgpu' && !params.has('gpu')) watchWebgpu();
  watchStartFailure();
  const mod = await import(`./pkg/client_${variant}.js`);
  await mod.default();
  return mod;
}

/** Start the Bevy app. winit hands control to the browser by throwing on purpose. */
function runClient(mod, config) {
  try {
    mod.client_start(config);
  } catch (e) {
    if (!String(e).includes('Using exceptions for control flow')) throw e;
  }
  document.getElementById('boot')?.remove();
}

// ---------- voice ----------

/** Positions and drunk pitch for proximity voice, read from the client's status object. */
function voicePositions() {
  const s = window.__lastCall ?? {};
  const others = new Map();
  for (const [id, x, y, z] of s.players ?? []) {
    if (id !== s.playerId) others.set(id, [x, y, z]);
  }
  // Drunk speakers sound lower (the game computes the factor per player).
  const pitch = new Map(s.game?.voicePitch ?? []);
  return { me: s.ownPos ?? null, others, pitch };
}

/** Start voice once the client knows its player id. `?novoice` turns it off. */
function startVoiceWhenJoined(code) {
  if (params.has('novoice')) return;
  const timer = setInterval(async () => {
    const playerId = window.__lastCall?.playerId;
    if (!playerId) return;
    clearInterval(timer);
    try {
      window.__lcVoice = await startVoice({ code, playerId, positions: voicePositions });
    } catch (e) {
      console.warn('voice failed to start', e);
    }
  }, 250);
}

// ---------- drunk screen blur ----------

// The game reports a blur strength (0 to 1) from the drunk meter. Bevy turns
// off depth of field on WebGL2, so the page blurs the whole canvas with CSS.
function watchDrunkBlur() {
  const canvas = document.getElementById('bevy');
  let shown = 0;
  setInterval(() => {
    const blur = window.__lastCall?.game?.drunk?.blur ?? 0;
    const px = Math.round(blur * 50) / 10; // up to 5 px, in 0.1 px steps
    if (px !== shown && canvas) {
      canvas.style.filter = px > 0 ? `blur(${px}px)` : '';
      shown = px;
    }
  }, 200);
}
watchDrunkBlur();

// ---------- host worker ----------

// Tick reports land in window.__hostTicks.
window.__hostTicks = [];
function startHostWorker() {
  const url = new URL('./host-worker.js', import.meta.url);
  // `?fast=N` runs shifts N times faster; `?preset=` picks a test start;
  // `?customers=bar` keeps every customer at the bar.
  for (const key of ['fast', 'preset', 'customers']) if (params.get(key)) url.searchParams.set(key, params.get(key));
  const worker = new Worker(url, { type: 'module' });
  worker.addEventListener('message', (e) => {
    if (e.data?.t === 'tick') window.__hostTicks.push(e.data);
    if (e.data?.t === 'audit') exposeAudit(e.data.run);
    if (e.data?.t === 'error') console.error('host worker stack', e.data.message, e.data.stack);
  });
  worker.onerror = (e) => console.error('host worker error', e.message);
  return worker;
}
window.startHostWorker = startHostWorker;

// The RNG audit log of this tab's room: `window.__lastCallAudit.export()`
// gives the JSONL text; the "RNG log" button saves it as a file.
function exposeAudit(run) {
  const load = () => import('./audit.js');
  window.__lastCallAudit = {
    run,
    export: async () => (await load()).exportRun(run),
    download: async () => (await load()).download(run),
  };
  if (!document.getElementById('audit-export') && !params.has('hostonly')) {
    const b = Object.assign(document.createElement('button'), { id: 'audit-export', textContent: 'RNG log' });
    b.title = 'Download this room\'s RNG audit log (JSONL). Check it with: tools replay FILE';
    b.addEventListener('click', () => window.__lastCallAudit.download());
    document.body.append(b);
  }
}

// ---------- modes ----------

async function runHost() {
  const mod = await loadClient();
  const worker = startHostWorker();

  // The host's own client talks to the Worker over a private port (peer 0).
  const channel = new MessageChannel();
  worker.postMessage({ t: 'local', port: channel.port2 }, [channel.port2]);
  channel.port1.onmessage = (e) => mod.client_deliver(new Uint8Array(e.data));
  globalThis.__clientOut = (bytes) => {
    const copy = bytes.slice();
    channel.port1.postMessage(copy.buffer, [copy.buffer]);
  };

  // Remote players: data channel ids <-> small numeric keys for the Worker.
  const keyOf = new Map();
  const idOf = new Map();
  let nextKey = 1;
  let room = null;

  const open = (code) =>
    hostRoom(code, {
      onPeer(id) {
        const key = nextKey++;
        keyOf.set(id, key);
        idOf.set(key, id);
        worker.postMessage({ t: 'peer', peer: key });
      },
      onPacket(id, bytes) {
        const key = keyOf.get(id);
        if (key !== undefined) worker.postMessage({ t: 'pkt', peer: key, data: bytes.buffer }, [bytes.buffer]);
      },
      onPeerLeft(id) {
        const key = keyOf.get(id);
        if (key === undefined) return;
        keyOf.delete(id);
        idOf.delete(key);
        worker.postMessage({ t: 'leave', peer: key });
      },
      onClosed(code) {
        // 4409: someone else holds this code. Pick another.
        if (code === 4409) start(newRoomCode());
      },
    });

  worker.addEventListener('message', (e) => {
    if (e.data?.t !== 'out' || !room) return;
    for (const [key, data] of e.data.items) {
      const id = idOf.get(key);
      if (id) room.send(id, data);
    }
  });

  const start = (code) => {
    room = open(code);
    const link = `${location.origin}${location.pathname}?room=${code}`;
    window.__lcRoom = { code, link, role: 'host' };
    show(banner, `Room ${code}  ·  invite: ${link}`);
    return code;
  };
  const code = start(params.get('code') ?? newRoomCode());
  runClient(mod, { online: true, code, uuid: playerUuid(), name: displayName(), nodraw: params.has('nodraw') });
  startVoiceWhenJoined(code);
}

async function runPlayer(code) {
  const mod = await loadClient();
  window.__lcRoom = { code, role: 'player' };
  show(banner, `Joining room ${code}…`);
  let started = false;
  const net = joinRoom(code, {
    onOpen() {
      show(banner, `Room ${code}`);
      if (!started) {
        started = true;
        runClient(mod, { online: true, code, uuid: playerUuid(), name: displayName(), nodraw: params.has('nodraw') });
        startVoiceWhenJoined(code);
      }
    },
    onPacket: (bytes) => mod.client_deliver(bytes),
    onHostLeft(reason) {
      window.__lcHostLeft = reason;
      show(banner, reason === 'host left' ? 'Host left. The bar is closed.' : `Could not join: ${reason}`);
      banner.classList.add('alert');
    },
  });
  globalThis.__clientOut = (bytes) => net.send(bytes);
}

async function runTitle() {
  const mod = await loadClient();
  show(lobby);
  document.getElementById('create')?.addEventListener('click', () => {
    location.search = '?create';
  });
  document.getElementById('join-form')?.addEventListener('submit', (e) => {
    e.preventDefault();
    const code = parseRoomCode(document.getElementById('join-code').value);
    if (code) location.search = `?room=${code}`;
  });
  runClient(mod, { online: false });
}

// Pointer lock for mouse look once the player clicks the game.
document.getElementById('bevy')?.addEventListener('click', (e) => e.target.requestPointerLock?.());

const fail = (e) => {
  console.error(e);
  show(document.getElementById('boot') ?? banner, `Failed to start: ${e}`);
  window.__lastCallError = String(e); // the accessor keeps only the first error
};

if (params.has('hostonly')) {
  window.__lastCallVariant = 'none';
  show(document.getElementById('boot'), 'Host worker only');
  startHostWorker();
} else if (params.has('room')) {
  const code = parseRoomCode(params.get('room'));
  if (code) runPlayer(code).catch(fail);
  else fail('that room link is not valid');
} else if (params.has('create')) {
  runHost().catch(fail);
} else {
  runTitle().catch(fail);
}
