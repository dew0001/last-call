// Proximity voice: a full WebRTC audio mesh between the players in a room.
//
// Signaling uses the Worker's voice mode (/room/CODE?role=voice&peer=ID),
// where ID is the player's game id, so voices can be matched to positions.
// Peers already in the room offer to a newcomer. Each remote voice plays
// through Web Audio: source -> pitch shift (drunk) -> gain (distance) ->
// analyser -> speakers. Volume: full at 2 m, silent at 14 m, linear between
// (plan section 8). A Sloppy or worse speaker sounds lower to everyone else
// (the listener applies it; the game says how much). Wall occlusion comes
// with the room layouts.
import { signalUrl } from './net.js';

const ICE_SERVERS = [{ urls: 'stun:stun.l.google.com:19302' }];
const FULL_VOLUME_M = 2;
const SILENT_M = 14;
const MAX_BITRATE = 32_000;
const UPDATE_MS = 100;

/** Gain for a listener-to-speaker distance in meters. */
export function distanceGain(d) {
  if (!Number.isFinite(d)) return 0;
  return Math.min(1, Math.max(0, 1 - (d - FULL_VOLUME_M) / (SILENT_M - FULL_VOLUME_M)));
}

/**
 * Start voice. `positions()` returns { me: [x, y, z] | null, others: Map<id, [x, y, z]>,
 * pitch?: Map<id, number> }. Returns a handle: { peers(), level(id), peakHz(id), gain(id), stop() }.
 */
export async function startVoice({ code, playerId, positions }) {
  const ctx = new AudioContext();
  // The pitch shifter runs in an AudioWorklet; without one, voices play unshifted.
  const worklet = ctx.audioWorklet
    ? ctx.audioWorklet.addModule(new URL('./pitch-worklet.js', import.meta.url)).then(
        () => true,
        (e) => (console.warn('voice: no pitch shifter', e), false),
      )
    : Promise.resolve(false);
  const resume = () => ctx.state !== 'running' && ctx.resume().catch(() => {});
  resume();
  addEventListener('pointerdown', resume);
  addEventListener('keydown', resume);

  let local = null;
  const settings = () => window.__lcSettings ?? { volume: 1, voice: 'open' };
  try {
    if (settings().voice === 'off') throw Object.assign(new Error('voice off in settings'), { name: 'Off' });
    local = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
  } catch (e) {
    // No microphone or permission denied: still hear everyone else.
    console.warn('voice: no microphone', e?.name ?? e);
  }

  const ws = new WebSocket(`${signalUrl()}/room/${code}?role=voice&peer=${playerId}`);
  const send = (obj) => ws.readyState === WebSocket.OPEN && ws.send(JSON.stringify(obj));
  const peers = new Map(); // id -> { pc, gain, analyser, el, buf }

  const drop = (id) => {
    const p = peers.get(id);
    if (!p) return;
    peers.delete(id);
    try {
      p.pc.close();
    } catch {}
    p.gain?.disconnect();
    p.shift?.disconnect();
    p.el?.remove();
  };

  const peer = (id) => {
    let p = peers.get(id);
    if (p) return p;
    const pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });
    p = { pc, gain: null, analyser: null, el: null, buf: null };
    peers.set(id, p);
    if (local) {
      for (const track of local.getAudioTracks()) {
        const sender = pc.addTrack(track, local);
        const params = sender.getParameters();
        params.encodings = [{ ...(params.encodings?.[0] ?? {}), maxBitrate: MAX_BITRATE }];
        sender.setParameters(params).catch(() => {});
      }
    } else {
      pc.addTransceiver('audio', { direction: 'recvonly' });
    }
    pc.onicecandidate = (e) => {
      if (e.candidate) send({ Signal: { receiver: id, data: { IceCandidate: JSON.stringify(e.candidate) } } });
    };
    pc.ontrack = async (e) => {
      const stream = e.streams[0] ?? new MediaStream([e.track]);
      const shift = (await worklet) ? new AudioWorkletNode(ctx, 'pitch-shift') : null;
      // Chrome only feeds remote WebRTC audio into Web Audio while a media
      // element plays it; keep a muted one.
      const el = new Audio();
      el.muted = true;
      el.srcObject = stream;
      el.play().catch(() => {});
      const source = ctx.createMediaStreamSource(stream);
      const gain = ctx.createGain();
      gain.gain.value = 0;
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 1024;
      (shift ? source.connect(shift).connect(gain) : source.connect(gain)).connect(analyser).connect(ctx.destination);
      Object.assign(p, { gain, shift, analyser, el, buf: new Float32Array(analyser.fftSize) });
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === 'failed' || pc.connectionState === 'closed') drop(id);
    };
    return p;
  };

  ws.onmessage = async (e) => {
    const msg = JSON.parse(e.data);
    if (msg.NewPeer) {
      const { pc } = peer(msg.NewPeer);
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      send({ Signal: { receiver: msg.NewPeer, data: { Offer: offer.sdp } } });
    } else if (msg.Signal) {
      const id = msg.Signal.sender;
      const data = msg.Signal.data;
      const { pc } = peer(id);
      if (data.Offer) {
        await pc.setRemoteDescription({ type: 'offer', sdp: data.Offer });
        const answer = await pc.createAnswer();
        await pc.setLocalDescription(answer);
        send({ Signal: { receiver: id, data: { Answer: answer.sdp } } });
      } else if (data.Answer) {
        await pc.setRemoteDescription({ type: 'answer', sdp: data.Answer });
      } else if (data.IceCandidate) {
        try {
          await pc.addIceCandidate(JSON.parse(data.IceCandidate));
        } catch {}
      }
    } else if (msg.PeerLeft) {
      drop(msg.PeerLeft);
    }
  };

  // Proximity: set each remote voice's volume from 3D distance.
  const timer = setInterval(() => {
    const { me, others, pitch } = positions();
    // Settings: master volume, and the microphone by voice mode.
    const { volume = 1, voice = 'open' } = settings();
    const talking = voice === 'open' || (voice === 'push' && window.__lcPushToTalk);
    for (const t of local?.getAudioTracks() ?? []) if (t.enabled !== talking) t.enabled = talking;
    for (const [id, p] of peers) {
      if (!p.gain) continue;
      const them = others.get(id);
      const d = me && them ? Math.hypot(me[0] - them[0], me[1] - them[1], me[2] - them[2]) : Infinity;
      p.gain.gain.setTargetAtTime(distanceGain(d) * volume, ctx.currentTime, 0.05);
      p.shift?.parameters.get('pitch').setValueAtTime(pitch?.get(id) ?? 1, ctx.currentTime);
    }
  }, UPDATE_MS);

  return {
    context: ctx,
    peers: () => [...peers.keys()],
    /** Strongest frequency (Hz) in a remote voice after its pitch shift. */
    peakHz(id) {
      const p = peers.get(id);
      if (!p?.analyser) return 0;
      const bins = new Float32Array(p.analyser.frequencyBinCount);
      p.analyser.getFloatFrequencyData(bins);
      let best = 1;
      for (let i = 2; i < bins.length; i++) if (bins[i] > bins[best]) best = i;
      return (best * ctx.sampleRate) / p.analyser.fftSize;
    },
    /** RMS level (0..1) of a remote voice after its distance gain. */
    level(id) {
      const p = peers.get(id);
      if (!p?.analyser) return 0;
      p.analyser.getFloatTimeDomainData(p.buf);
      let sum = 0;
      for (const v of p.buf) sum += v * v;
      return Math.sqrt(sum / p.buf.length);
    },
    gain: (id) => peers.get(id)?.gain?.gain.value ?? 0,
    stop() {
      clearInterval(timer);
      for (const id of [...peers.keys()]) drop(id);
      ws.close();
      local?.getTracks().forEach((t) => t.stop());
      ctx.close();
    },
  };
}
