// WebRTC game transport. Star topology: every player connects to the host.
//
// Signaling goes through the Worker's /room/CODE WebSocket, which speaks the
// matchbox JSON protocol (IdAssigned, NewPeer, PeerLeft, Signal). The host is
// the offerer for every player. Game bytes travel on one unordered,
// no-retransmit data channel per player (lightyear does its own reliability).
//
// Everything here is event driven, so the host keeps relaying packets while
// its tab is hidden and animation frames are paused.

const ICE_SERVERS = [{ urls: 'stun:stun.l.google.com:19302' }];
const CODE_LETTERS = 'BCDFGHJKLMNPQRSTVWXYZ';

/** A random room code: 5 letters, no vowels, never O or I. */
export function newRoomCode() {
  const bytes = crypto.getRandomValues(new Uint8Array(5));
  return Array.from(bytes, (b) => CODE_LETTERS[b % CODE_LETTERS.length]).join('');
}

/** Normalize a room code from a link, or null if it is not one. */
export function parseRoomCode(raw) {
  const code = String(raw ?? '').trim().toUpperCase();
  return code.length === 5 && [...code].every((c) => CODE_LETTERS.includes(c)) ? code : null;
}

/** Signaling Worker base URL (ws or wss). `?signal=` overrides it for tests. */
export function signalUrl() {
  const override = new URLSearchParams(location.search).get('signal');
  if (override) return override.replace(/\/$/, '');
  if (location.hostname === 'localhost' || location.hostname === '127.0.0.1') return 'ws://127.0.0.1:8787';
  return 'wss://last-call-signal.drewduncanjr.workers.dev';
}

// Firefox throws while a channel is closing. Game packets are unreliable by
// design, so a dropped one is fine.
function trySend(dc, bytes) {
  try {
    dc.send(bytes);
  } catch {}
}

function openSignal(code, role) {
  const ws = new WebSocket(`${signalUrl()}/room/${code}?role=${role}`);
  const send = (obj) => ws.readyState === WebSocket.OPEN && ws.send(JSON.stringify(obj));
  return { ws, send };
}

function wirePeerConnection(pc, sendSignal) {
  pc.onicecandidate = (e) => {
    if (e.candidate) sendSignal({ IceCandidate: JSON.stringify(e.candidate) });
  };
}

async function applySignal(pc, data, sendSignal) {
  if (data.Offer) {
    await pc.setRemoteDescription({ type: 'offer', sdp: data.Offer });
    const answer = await pc.createAnswer();
    await pc.setLocalDescription(answer);
    sendSignal({ Answer: answer.sdp });
  } else if (data.Answer) {
    await pc.setRemoteDescription({ type: 'answer', sdp: data.Answer });
  } else if (data.IceCandidate) {
    try {
      await pc.addIceCandidate(JSON.parse(data.IceCandidate));
    } catch {
      // Candidates can arrive after the connection closed; ignore them.
    }
  }
}

/**
 * Host a room. Callbacks:
 *   onPeer(peerId)            a player's data channel opened
 *   onPacket(peerId, bytes)   bytes from a player (Uint8Array)
 *   onPeerLeft(peerId)        a player left or its channel failed
 *   onClosed(code, reason)    signaling closed (4409 means the code is taken)
 * Returns { code, send(peerId, bytes), close(), peers() }.
 */
export function hostRoom(code, { onPeer, onPacket, onPeerLeft, onClosed }) {
  const { ws, send } = openSignal(code, 'host');
  const peers = new Map(); // peerId -> { pc, dc }

  const drop = (id) => {
    const p = peers.get(id);
    if (!p) return;
    peers.delete(id);
    try {
      p.pc.close();
    } catch {}
    onPeerLeft?.(id);
  };

  ws.onmessage = async (e) => {
    const msg = JSON.parse(e.data);
    if (msg.NewPeer) {
      const id = msg.NewPeer;
      const pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });
      const sendSignal = (data) => send({ Signal: { receiver: id, data } });
      wirePeerConnection(pc, sendSignal);
      const dc = pc.createDataChannel('game', { ordered: false, maxRetransmits: 0 });
      dc.binaryType = 'arraybuffer';
      dc.onopen = () => onPeer?.(id);
      dc.onmessage = (ev) => onPacket?.(id, new Uint8Array(ev.data));
      dc.onclose = () => drop(id);
      pc.onconnectionstatechange = () => {
        if (pc.connectionState === 'failed' || pc.connectionState === 'closed') drop(id);
      };
      peers.set(id, { pc, dc });
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      sendSignal({ Offer: offer.sdp });
    } else if (msg.Signal) {
      const p = peers.get(msg.Signal.sender);
      if (p) await applySignal(p.pc, msg.Signal.data, (data) => send({ Signal: { receiver: msg.Signal.sender, data } }));
    } else if (msg.PeerLeft) {
      // Signaling noticed the player left. Keep the channel if it still works
      // (the player may only have lost its WebSocket); drop it otherwise.
      const p = peers.get(msg.PeerLeft);
      if (p && p.dc.readyState !== 'open') drop(msg.PeerLeft);
    }
  };
  ws.onclose = (e) => onClosed?.(e.code, e.reason);

  return {
    code,
    send(id, bytes) {
      const p = peers.get(id);
      if (p && p.dc.readyState === 'open') trySend(p.dc, bytes);
    },
    peers: () => [...peers.keys()],
    close() {
      for (const id of [...peers.keys()]) drop(id);
      ws.close();
    },
  };
}

/**
 * Join a room. Callbacks:
 *   onOpen()            the data channel to the host opened
 *   onPacket(bytes)     bytes from the host (Uint8Array)
 *   onHostLeft(reason)  the host left, refused us, or the channel died
 * Returns { send(bytes), close() }.
 */
export function joinRoom(code, { onOpen, onPacket, onHostLeft }) {
  const { ws, send } = openSignal(code, 'player');
  let pc = null;
  let dc = null;
  let hostId = null;
  let ended = false;

  const end = (reason) => {
    if (ended) return;
    ended = true;
    try {
      pc?.close();
    } catch {}
    try {
      ws.close();
    } catch {}
    onHostLeft?.(reason);
  };

  ws.onmessage = async (e) => {
    const msg = JSON.parse(e.data);
    if (msg.Signal) {
      hostId = msg.Signal.sender;
      const sendSignal = (data) => send({ Signal: { receiver: hostId, data } });
      if (!pc) {
        pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });
        wirePeerConnection(pc, sendSignal);
        pc.ondatachannel = (ev) => {
          dc = ev.channel;
          dc.binaryType = 'arraybuffer';
          dc.onopen = () => onOpen?.();
          dc.onmessage = (m) => onPacket?.(new Uint8Array(m.data));
          dc.onclose = () => end('host left');
        };
        pc.onconnectionstatechange = () => {
          if (pc.connectionState === 'failed' || pc.connectionState === 'closed') end('connection lost');
        };
      }
      await applySignal(pc, msg.Signal.data, sendSignal);
    } else if (msg.PeerLeft && msg.PeerLeft === hostId) {
      end('host left');
    }
  };
  ws.onclose = (e) => {
    if (e.code === 4404) end('no such room');
    else if (e.code === 4410 || e.code === 4000) end('host left');
    else if (e.code === 4429) end('room full');
    // Other closes (network blips) leave an open data channel alone.
  };

  return {
    send(bytes) {
      if (dc && dc.readyState === 'open') trySend(dc, bytes);
    },
    close() {
      end('left');
    },
  };
}
