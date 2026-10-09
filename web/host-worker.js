// Boots the host simulation wasm inside a dedicated module Web Worker and
// moves packets between it and the page.
//
// Page -> worker messages:
//   { t: 'peer', peer }            a player's data channel opened
//   { t: 'pkt', peer, data }       bytes from a player (ArrayBuffer)
//   { t: 'leave', peer }           a player left
//   { t: 'local', port }           MessagePort for the host's own client (peer 0)
// Worker -> page messages:
//   { t: 'tick', tick, tps }       once per second
//   { t: 'out', items: [[peer, ArrayBuffer], ...] }  bytes for players
import init, { host_worker_start, host_connect, host_packet, host_leave } from './pkg/host.js';

const LOCAL_PEER = 0;
let localPort = null;
let outbox = [];
let flushQueued = false;

// Called by the wasm host for every packet it sends.
globalThis.__hostOut = (peer, bytes) => {
  if (peer === LOCAL_PEER) {
    if (localPort) {
      const copy = bytes.slice();
      localPort.postMessage(copy.buffer, [copy.buffer]);
    }
    return;
  }
  outbox.push([peer, bytes.slice().buffer]);
  if (!flushQueued) {
    flushQueued = true;
    queueMicrotask(() => {
      const items = outbox;
      outbox = [];
      flushQueued = false;
      postMessage({ t: 'out', items }, items.map(([, b]) => b));
    });
  }
};

const pending = [];
let ready = false;

function handle(msg) {
  switch (msg.t) {
    case 'peer':
      host_connect(msg.peer);
      break;
    case 'pkt':
      host_packet(msg.peer, new Uint8Array(msg.data));
      break;
    case 'leave':
      host_leave(msg.peer);
      break;
    case 'local':
      localPort = msg.port;
      host_connect(LOCAL_PEER);
      localPort.onmessage = (e) => host_packet(LOCAL_PEER, new Uint8Array(e.data));
      break;
  }
}

// Uncaught errors (a wasm trap in the tick loop) go to the page with their
// stack, so a test log shows where the host died.
addEventListener('error', (e) => {
  postMessage({ t: 'error', message: String(e.message), stack: String(e.error?.stack ?? '') });
});

onmessage = (e) => (ready ? handle(e.data) : pending.push(e.data));

await init();
// `?fast=N` on the worker URL shortens every shift phase N times (tests).
const fast = Number(new URL(self.location.href).searchParams.get('fast') ?? 1) || 1;
host_worker_start(fast, crypto.getRandomValues(new Uint8Array(32)));
ready = true;
for (const m of pending.splice(0)) handle(m);
