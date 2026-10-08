// Integration test for the signaling Worker under `wrangler dev`.
// Run: SIGNAL_URL=ws://127.0.0.1:8787 node --test tests/signal/
import { test } from 'node:test';
import assert from 'node:assert/strict';

const BASE = process.env.SIGNAL_URL ?? 'ws://127.0.0.1:8787';
const LETTERS = 'BCDFGHJKLMNPQRSTVWXYZ';
const code = () => Array.from({ length: 5 }, () => LETTERS[Math.floor(Math.random() * LETTERS.length)]).join('');

// A WebSocket with an awaitable message queue.
function connect(room, role) {
  const ws = new WebSocket(`${BASE}/room/${room}?role=${role}`);
  const queue = [];
  const waiters = [];
  const closed = new Promise((resolve) => ws.addEventListener('close', (e) => resolve({ code: e.code, reason: e.reason })));
  ws.addEventListener('message', (e) => {
    const msg = JSON.parse(e.data);
    const w = waiters.shift();
    w ? w(msg) : queue.push(msg);
  });
  const next = (ms = 5000) =>
    queue.length
      ? Promise.resolve(queue.shift())
      : new Promise((resolve, reject) => {
          const t = setTimeout(() => reject(new Error(`no message within ${ms} ms`)), ms);
          waiters.push((m) => {
            clearTimeout(t);
            resolve(m);
          });
        });
  return { ws, next, closed, send: (obj) => ws.send(JSON.stringify(obj)) };
}

test('host and player are introduced and can exchange signals', async () => {
  const room = code();
  const host = connect(room, 'host');
  const hostId = (await host.next()).IdAssigned;
  assert.ok(hostId);

  const player = connect(room, 'player');
  const playerId = (await player.next()).IdAssigned;
  assert.deepEqual(await host.next(), { NewPeer: playerId });

  host.send({ Signal: { receiver: playerId, data: { Offer: 'sdp-offer' } } });
  assert.deepEqual(await player.next(), { Signal: { sender: hostId, data: { Offer: 'sdp-offer' } } });
  player.send({ Signal: { receiver: hostId, data: { Answer: 'sdp-answer' } } });
  assert.deepEqual(await host.next(), { Signal: { sender: playerId, data: { Answer: 'sdp-answer' } } });

  player.ws.close();
  assert.deepEqual(await host.next(), { PeerLeft: playerId });
  host.ws.close();
});

test('players never see each other', async () => {
  const room = code();
  const host = connect(room, 'host');
  await host.next();
  const a = connect(room, 'player');
  const aId = (await a.next()).IdAssigned;
  const b = connect(room, 'player');
  await b.next();
  b.send({ Signal: { receiver: aId, data: 'sneaky' } });
  await assert.rejects(a.next(800));
  for (const s of [a, b, host]) s.ws.close();
});

test('joining a room with no host is refused', async () => {
  const p = connect(code(), 'player');
  assert.equal((await p.closed).code, 4404);
});

test('a second host for the same code is refused', async () => {
  const room = code();
  const host = connect(room, 'host');
  await host.next();
  const dup = connect(room, 'host');
  assert.equal((await dup.closed).code, 4409);
  host.ws.close();
});

test('host leaving ends the room for everyone', async () => {
  const room = code();
  const host = connect(room, 'host');
  const hostId = (await host.next()).IdAssigned;
  const p = connect(room, 'player');
  await p.next();
  await host.next(); // NewPeer
  host.ws.close();
  assert.deepEqual(await p.next(), { PeerLeft: hostId });
  assert.equal((await p.closed).code, 4000);
  const late = connect(room, 'player');
  assert.equal((await late.closed).code, 4410);
});

test('bad room codes are rejected', async () => {
  const res = await fetch(`${BASE.replace('ws', 'http')}/room/AEIOU?role=player`);
  assert.equal(res.status, 400);
});
