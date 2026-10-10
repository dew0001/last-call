// Sound: effects, ambience and the jukebox, all synthesized with WebAudio
// (no sound files to download).
//
// The game status (`window.__lastCall`, 20 updates a second) is compared
// with the last one to find what just happened: a step, a pour, a sale, a
// coin, a shift bell, a spinning reel, a chaos event. The crowd murmur grows
// with the customers in the bar; outside, wind and waves take over; the
// jukebox plays in the main bar. Levels come from the settings' Audio tab
// (`window.__lcSettings`: volume, sfx, music).

let ctx = null;
let master, sfxBus, musicBus, ambBus, noise;

function start() {
  if (ctx) return;
  const AC = window.AudioContext ?? window.webkitAudioContext;
  if (!AC) return;
  ctx = new AC();
  master = ctx.createGain();
  master.connect(ctx.destination);
  sfxBus = gain(master, 0.9);
  musicBus = gain(master, 0.5);
  ambBus = gain(master, 0.8);
  // Two seconds of white noise, reused by every noisy sound.
  noise = ctx.createBuffer(1, ctx.sampleRate * 2, ctx.sampleRate);
  const d = noise.getChannelData(0);
  for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  startAmbience();
  startJukebox();
}
// Browsers start audio only after a click or a key.
for (const ev of ['pointerdown', 'keydown']) addEventListener(ev, () => { start(); ctx?.resume(); }, { capture: true });

function gain(to, value) {
  const g = ctx.createGain();
  g.gain.value = value;
  g.connect(to);
  return g;
}

const now = () => ctx.currentTime;

/** A decaying sine ping. */
function ping(freq, { at = 0, dur = 0.4, vol = 0.3, type = 'sine', bus = sfxBus, slide = 0 } = {}) {
  const t = now() + at;
  const o = ctx.createOscillator();
  o.type = type;
  o.frequency.setValueAtTime(freq, t);
  if (slide) o.frequency.exponentialRampToValueAtTime(Math.max(20, freq * slide), t + dur);
  const g = ctx.createGain();
  g.gain.setValueAtTime(0.0001, t);
  g.gain.exponentialRampToValueAtTime(vol, t + 0.005);
  g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
  o.connect(g).connect(bus);
  o.start(t);
  o.stop(t + dur + 0.05);
}

/** A filtered noise burst. */
function burst({ at = 0, dur = 0.08, vol = 0.3, freq = 800, q = 1, type = 'lowpass', bus = sfxBus } = {}) {
  const t = now() + at;
  const src = ctx.createBufferSource();
  src.buffer = noise;
  src.playbackRate.value = 0.8 + Math.random() * 0.4;
  const f = ctx.createBiquadFilter();
  f.type = type;
  f.frequency.value = freq;
  f.Q.value = q;
  const g = ctx.createGain();
  g.gain.setValueAtTime(vol, t);
  g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
  src.connect(f).connect(g).connect(bus);
  src.start(t, Math.random());
  src.stop(t + dur + 0.05);
}

// ---------- effects ----------

export const sfx = {
  step(surface) {
    const hard = surface === 'outside';
    burst({ dur: 0.07, vol: 0.22, freq: hard ? 1800 : 520, q: 0.8 });
    ping(hard ? 140 : 95, { dur: 0.06, vol: 0.12, slide: 0.6 });
  },
  clink() {
    ping(2650 + Math.random() * 200, { dur: 0.35, vol: 0.12 });
    ping(4100 + Math.random() * 300, { at: 0.01, dur: 0.25, vol: 0.06 });
  },
  coin() {
    ping(1760, { dur: 0.12, vol: 0.12, type: 'square' });
    ping(2350, { at: 0.07, dur: 0.35, vol: 0.12, type: 'square' });
  },
  till() {
    burst({ dur: 0.12, vol: 0.25, freq: 3000, type: 'bandpass', q: 2 });
    for (const [f, at] of [[2093, 0.06], [2637, 0.06], [3136, 0.06], [4186, 0.08]]) ping(f, { at, dur: 0.9, vol: 0.08 });
  },
  bell(times = 1, freq = 880) {
    for (let i = 0; i < times; i++) {
      ping(freq, { at: i * 0.45, dur: 1.4, vol: 0.18 });
      ping(freq * 2.76, { at: i * 0.45, dur: 0.6, vol: 0.05 });
    }
  },
  doorbell() {
    ping(988, { dur: 0.9, vol: 0.2 });
    ping(784, { at: 0.35, dur: 1.2, vol: 0.2 });
  },
  doom() {
    ping(110, { dur: 2.2, vol: 0.3, type: 'sawtooth', slide: 0.7 });
    ping(82, { at: 0.1, dur: 2.4, vol: 0.25, type: 'triangle', slide: 0.7 });
  },
  alarm() {
    for (let i = 0; i < 4; i++) ping(i % 2 ? 660 : 880, { at: i * 0.22, dur: 0.2, vol: 0.12, type: 'square' });
  },
  reel() {
    for (let i = 0; i < 6; i++) burst({ at: i * 0.07, dur: 0.03, vol: 0.12, freq: 2500, type: 'bandpass', q: 4 });
  },
  win() {
    [523, 659, 784, 1047].forEach((f, i) => ping(f, { at: i * 0.09, dur: 0.4, vol: 0.14, type: 'triangle' }));
  },
  thud() {
    ping(70, { dur: 0.35, vol: 0.5, slide: 0.5 });
    burst({ dur: 0.2, vol: 0.3, freq: 300 });
  },
  gulp() {
    ping(300, { dur: 0.12, vol: 0.15, slide: 0.5 });
    ping(260, { at: 0.18, dur: 0.12, vol: 0.12, slide: 0.5 });
  },
  click() {
    burst({ dur: 0.03, vol: 0.12, freq: 4000, type: 'highpass' });
  },
};
window.__lcSfx = sfx;

// The beer tap: hiss and gurgle while a pour runs.
let pourNode = null;
function setPouring(on) {
  if (on && !pourNode) {
    const src = ctx.createBufferSource();
    src.buffer = noise;
    src.loop = true;
    const f = ctx.createBiquadFilter();
    f.type = 'bandpass';
    f.frequency.value = 900;
    f.Q.value = 1.5;
    const lfo = ctx.createOscillator();
    lfo.frequency.value = 7;
    const depth = gain(f.frequency, 350);
    lfo.connect(depth);
    const g = gain(sfxBus, 0.0001);
    g.gain.exponentialRampToValueAtTime(0.18, now() + 0.1);
    src.connect(f).connect(g);
    src.start();
    lfo.start();
    pourNode = { src, lfo, g };
  } else if (!on && pourNode) {
    const { src, lfo, g } = pourNode;
    g.gain.exponentialRampToValueAtTime(0.0001, now() + 0.15);
    src.stop(now() + 0.2);
    lfo.stop(now() + 0.2);
    pourNode = null;
  }
}

// ---------- ambience ----------

let crowd, outside;
function startAmbience() {
  // Crowd: noise in the voice band with a slow swell.
  const make = (freq, q, bus) => {
    const src = ctx.createBufferSource();
    src.buffer = noise;
    src.loop = true;
    const f = ctx.createBiquadFilter();
    f.type = 'bandpass';
    f.frequency.value = freq;
    f.Q.value = q;
    const g = gain(bus, 0);
    const lfo = ctx.createOscillator();
    lfo.frequency.value = 0.13;
    lfo.connect(gain(g.gain, 0.02));
    src.connect(f).connect(g);
    src.start();
    lfo.start();
    return g;
  };
  crowd = make(650, 0.7, ambBus);
  outside = make(380, 0.4, ambBus);
}

// ---------- jukebox ----------

// A slow bar-room loop: Am F C G, a soft pad, a walking bass and brushed drums.
const CHORDS = [
  [57, 60, 64],
  [53, 57, 60],
  [48, 52, 55],
  [55, 59, 62],
];
const hz = (m) => 440 * 2 ** ((m - 69) / 12);
let jukeboxIn = null;
function startJukebox() {
  jukeboxIn = gain(musicBus, 0);
  const lp = ctx.createBiquadFilter();
  lp.type = 'lowpass';
  lp.frequency.value = 2200;
  lp.connect(jukeboxIn);
  const beat = 60 / 84;
  let next = now() + 0.2;
  let bar = 0;
  const schedule = () => {
    while (next < now() + 1.0) {
      const chord = CHORDS[bar % 4];
      for (const m of chord) {
        const o = ctx.createOscillator();
        o.type = 'triangle';
        o.frequency.value = hz(m);
        const g = ctx.createGain();
        g.gain.setValueAtTime(0.0001, next);
        g.gain.linearRampToValueAtTime(0.05, next + 0.3);
        g.gain.linearRampToValueAtTime(0.0001, next + beat * 4);
        o.connect(g).connect(lp);
        o.start(next);
        o.stop(next + beat * 4 + 0.05);
      }
      for (let b = 0; b < 4; b++) {
        const t = next + b * beat;
        const root = chord[0] - 24 + [0, 7, 12, 7][b];
        const o = ctx.createOscillator();
        o.type = 'sine';
        o.frequency.value = hz(root);
        const g = ctx.createGain();
        g.gain.setValueAtTime(0.0001, t);
        g.gain.exponentialRampToValueAtTime(0.16, t + 0.02);
        g.gain.exponentialRampToValueAtTime(0.0001, t + beat * 0.9);
        o.connect(g).connect(lp);
        o.start(t);
        o.stop(t + beat);
        // Kick on 1 and 3, brush on every beat.
        if (b % 2 === 0) {
          const k = ctx.createOscillator();
          k.frequency.setValueAtTime(120, t);
          k.frequency.exponentialRampToValueAtTime(45, t + 0.12);
          const kg = ctx.createGain();
          kg.gain.setValueAtTime(0.25, t);
          kg.gain.exponentialRampToValueAtTime(0.0001, t + 0.18);
          k.connect(kg).connect(lp);
          k.start(t);
          k.stop(t + 0.2);
        }
        const src = ctx.createBufferSource();
        src.buffer = noise;
        const f = ctx.createBiquadFilter();
        f.type = 'highpass';
        f.frequency.value = 5000;
        const bg = ctx.createGain();
        bg.gain.setValueAtTime(b % 2 ? 0.05 : 0.025, t + beat / 2);
        bg.gain.exponentialRampToValueAtTime(0.0001, t + beat / 2 + 0.12);
        src.connect(f).connect(bg).connect(lp);
        src.start(t + beat / 2, Math.random());
        src.stop(t + beat / 2 + 0.15);
      }
      next += beat * 4;
      bar += 1;
    }
  };
  setInterval(schedule, 250);
}

// ---------- watching the game ----------

const inBar = (p) => p && p[0] > -10 && p[0] < 10 && p[2] > -7 && p[2] < 7;
const outdoors = (p) => p && (p[2] > 7 || p[0] > 10);
let last = null;
let stepDist = 0;
let lastPos = null;

function watch() {
  const s = window.__lastCall;
  if (!ctx || !s?.playerId || !s.game) {
    last = null;
    if (pourNode) setPouring(false);
    return;
  }
  const set = window.__lcSettings ?? {};
  const vol = set.volume ?? 1;
  master.gain.setTargetAtTime(vol, now(), 0.1);
  sfxBus.gain.setTargetAtTime(set.sfx ?? 0.9, now(), 0.1);
  musicBus.gain.setTargetAtTime(set.music ?? 0.5, now(), 0.1);
  const g = s.game;
  const pos = s.ownPos;

  // Steps: one every 0.7 m walked.
  if (pos && lastPos) {
    const d = Math.hypot(pos[0] - lastPos[0], pos[2] - lastPos[2]);
    if (d < 2) stepDist += d;
    if (stepDist > 0.7) {
      stepDist = 0;
      sfx.step(outdoors(pos) ? 'outside' : 'inside');
    }
  }
  lastPos = pos;

  // Ambience follows where you are; the crowd grows with customers.
  const customers = g.customers?.length ?? 0;
  const out = outdoors(pos);
  crowd.gain.setTargetAtTime(out ? 0.004 : Math.min(0.06, 0.008 + customers * 0.004), now(), 0.8);
  outside.gain.setTargetAtTime(out ? 0.035 : 0.0, now(), 0.8);
  jukeboxIn.gain.setTargetAtTime(inBar(pos) ? 1 : out ? 0.08 : 0.35, now(), 0.6);

  setPouring(!!g.pour);
  if (last) {
    const lg = last.game;
    if (g.beer && !lg.beer) sfx.clink();
    if (!g.beer && lg.beer && (g.drunk?.level ?? 0) > (lg.drunk?.level ?? 0)) sfx.gulp();
    if ((g.money?.house ?? 0) > (lg.money?.house ?? 0)) sfx.till();
    if ((g.pocket ?? 0) > (lg.pocket ?? 0)) sfx.coin();
    if (g.shift && lg.shift && g.shift.phase !== lg.shift.phase) {
      ({ OPEN: () => sfx.doorbell(), 'LAST CALL': () => sfx.bell(3), PAYMENT: () => sfx.doom(), SETUP: () => sfx.bell(1, 660) })[
        g.shift.phase
      ]?.();
    }
    if (g.drunk?.passedOut && !lg.drunk?.passedOut) sfx.thud();
    const slots = g.casino?.slots ?? [];
    slots.forEach((m, i) => {
      const was = lg.casino?.slots?.[i];
      if (m.spinning && !was?.spinning) sfx.reel();
      if (was?.spinning && !m.spinning && m.lastReturn > m.lastBet) sfx.win();
    });
    if ((g.phase4?.chaos?.length ?? 0) > (lg.phase4?.chaos?.length ?? 0)) sfx.alarm();
  }
  last = s;
}
setInterval(watch, 100);

// Menu buttons click.
addEventListener('click', (e) => {
  if (ctx && e.target instanceof HTMLButtonElement) sfx.click();
});
