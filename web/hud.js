import { toast } from './settings.js';

// The in-game HUD, drawn by the page over the canvas from the game status
// (`window.__lastCall`, published by the client 20 times a second):
//   top left:     week and shift, the phase and its clock
//   top right:    the house pool, your pocket, the debt and this week's due
//   bottom left:  the drunk meter
//   bottom right: the table, fixture or minigame you stand at
//   top center:   chaos events; bottom center: the pour gauge
//   full screen:  the end of a run
// The pour gauge and the end-of-run screen stay in the game.

const TRACKS = ['Last Orders', 'Tide Out', 'Neon Rain', 'Loan Shark Blues', 'Closing Time'];
const PHASES = { SETUP: 'Setup', OPEN: 'Open', 'LAST CALL': 'Last call', PAYMENT: 'Payment' };
const DRUNK_COLORS = ['#6fd4ff', '#ffd25a', '#ff9a3c', '#ff4f6d', '#c04dff'];

// Places with an action (crates/shared/src/bar.rs and world.rs): x, z, reach.
const TAP = [4.0, -4.0, 1.4];
const SAFE = [9.4, -6.4, 1.3];

/** A key's label from the player's bindings (web/settings.js). */
const key = (action) => {
  const code = window.__lcSettings?.keys?.[action] ?? '';
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  return code.replace(/(Left|Right)$/, '') || '?';
};
const near = (pos, [x, z, r]) => pos && Math.hypot(pos[0] - x, pos[2] - z) < r;

/** What the player can do right now, as one line (or ''). */
function prompt(s) {
  const g = s.game;
  if (g.drunk?.passedOut) return 'You passed out. Someone has to drag you somewhere safe…';
  if (g.pour) return '';
  if (g.beer) {
    return `Serve it: walk to a waiting customer and press [${key('drop')}] by the counter · [${key('use')}] drink it yourself`;
  }
  if (s.holding) return `[${key('throw')}] hold to throw · [${key('drop')}] drop`;
  if (near(s.ownPos, TAP)) return `Hold [${key('interact')}] to pour a beer, let go in the green`;
  if (near(s.ownPos, SAFE)) return `[${key('interact')}] put $100 from your pocket into the house pool`;
  return '';
}

const esc = (t) => t.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]);

/**
 * The table card from the game's panel text (crates/client/src/casino.rs):
 * the first line is the title, then the table's state, then (after a blank
 * line) the keys, drawn as keycaps.
 */
function tableCard(text) {
  const [head, ...rest] = text.split('\n');
  const gap = rest.indexOf('');
  const body = gap < 0 ? rest : rest.slice(0, gap);
  const keys = gap < 0 ? [] : rest.slice(gap + 1);
  // The name runs to a double space or a bracket: "BLACKJACK  (round 2)".
  const m = head.match(/^(.+?)(?:\s{2,}|\s(?=\()|$)(.*)$/) ?? [head, head, ''];
  const caps = (line) =>
    esc(line).replace(/(^|\s)(LMB|RMB|Wheel|Enter|[A-Z0-9]|[A-Z]\/[A-Z])(?=\s|$)/g, (_, pre, k) => `${pre}<kbd>${k}</kbd>`);
  // Menu lines start with their key: "1  Buy a pouch", "U  use".
  const row = (l) => esc(l).replace(/^([0-9A-Z])  /, '<kbd>$1</kbd> ');
  return (
    `<div class="hud-table-title"><span>${esc(m[1])}</span><small>${esc(m[2])}</small></div>` +
    body.filter((l) => l.trim()).map((l) => `<div class="hud-table-row">${row(l)}</div>`).join('') +
    (keys.length ? `<div class="hud-table-keys">${keys.map(caps).join('<br>')}</div>` : '')
  );
}

const money = (v) => `${v < 0 ? '-' : ''}$${Math.abs(v).toLocaleString('en-US')}`;
const clock = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;

export function mountHud() {
  if (document.getElementById('hud')) return;
  const hud = document.createElement('div');
  hud.id = 'hud';
  hud.hidden = true;
  hud.innerHTML = `
    <div class="hud-card hud-shift">
      <div class="hud-label" id="hud-week"></div>
      <div class="hud-row"><span class="hud-phase" id="hud-phase"></span><span class="hud-clock" id="hud-clock"></span></div>
    </div>
    <div class="hud-card hud-money">
      <div class="hud-money-row"><span class="hud-label">House</span><span id="hud-house" class="hud-big"></span></div>
      <div class="hud-money-row"><span class="hud-label">Pocket</span><span id="hud-pocket"></span></div>
      <div class="hud-debt"><div class="hud-debt-fill" id="hud-debt-fill"></div></div>
      <div class="hud-small" id="hud-debt"></div>
      <div class="hud-small" id="hud-due"></div>
    </div>
    <div class="hud-prompt" id="hud-prompt" hidden></div>
    <div class="hud-cards">
      <div class="hud-card hud-table" id="hud-game" hidden></div>
      <div class="hud-card hud-table hud-fixture" id="hud-fixture" hidden></div>
      <div class="hud-card hud-table" id="hud-table" hidden></div>
    </div>
    <div class="hud-chaos" id="hud-chaos" hidden></div>
    <div class="hud-focus" id="hud-focus" hidden></div>
    <div class="hud-pour" id="hud-pour" hidden>
      <div class="hud-label">Pour · let go in the green</div>
      <div class="hud-pour-track"><div class="hud-pour-green"></div><div class="hud-pour-fill" id="hud-pour-fill"></div></div>
      <div class="hud-pour-foam"><div id="hud-pour-foam"></div></div>
    </div>
    <div class="hud-outcome" id="hud-outcome" hidden></div>
    <div class="hud-card hud-drunk">
      <div class="hud-row"><span class="hud-label">Drunk</span><span id="hud-tier"></span></div>
      <div class="hud-meter"><div class="hud-meter-fill" id="hud-drunk-fill"></div></div>
    </div>`;
  document.body.append(hud);
  const $ = (id) => document.getElementById(id);
  const set = (id, text) => {
    const el = $(id);
    if (el.textContent !== text) el.textContent = text;
  };

  setInterval(() => {
    const s = window.__lastCall;
    const g = s?.game;
    const show = !!s?.playerId && !!g?.shift;
    hud.hidden = !show;
    document.body.classList.toggle('in-room', show);
    if (!show) return;
    const sh = g.shift;
    set('hud-week', `Week ${sh.week} · Shift ${sh.shift}/3`);
    set('hud-phase', PHASES[sh.phase] ?? sh.phase);
    $('hud-phase').dataset.phase = sh.phase;
    set('hud-clock', sh.waiting ? 'waiting' : clock(sh.secondsLeft) + (sh.running ? '' : ' ⏸'));

    const m = g.money;
    if (m) {
      set('hud-house', money(m.house));
      set('hud-debt', `Debt paid ${money(m.paid)} of ${money(m.debt)}`);
      set('hud-due', `Due this week ${money(m.due)}`);
      $('hud-debt-fill').style.width = `${Math.min(100, (100 * m.paid) / Math.max(1, m.debt)).toFixed(1)}%`;
      $('hud-due').classList.toggle('warn', m.house < m.due);
    }
    set('hud-pocket', money(g.pocket ?? 0));

    const line = prompt(s);
    $('hud-prompt').hidden = !line;
    set('hud-prompt', line);

    const panels = s.panels ?? {};
    const card = (id, text) => {
      const el = $(id);
      el.hidden = !text;
      if (text && el.dataset.text !== text) {
        el.dataset.text = text;
        el.innerHTML = tableCard(text);
      }
    };
    card('hud-table', s.tablePanel ?? '');
    card('hud-fixture', panels.fixture ?? '');
    card('hud-game', panels.game ?? '');

    // Chaos: one block per event, its name over its hint.
    const chaos = panels.chaos ?? '';
    const ch = $('hud-chaos');
    ch.hidden = !chaos;
    if (chaos && ch.dataset.text !== chaos) {
      ch.dataset.text = chaos;
      const lines = chaos.split('\n');
      ch.innerHTML = lines
        .map((l, i) => (i % 2 === 0 ? `<div class="hud-chaos-name">${esc(l)}</div>` : `<div class="hud-chaos-hint">${esc(l)}</div>`))
        .join('');
    }
    const focus = panels.focus ?? '';
    $('hud-focus').hidden = !focus;
    set('hud-focus', focus);

    const pour = g.pour;
    $('hud-pour').hidden = !pour;
    if (pour) {
      $('hud-pour-fill').style.width = `${Math.min(100, pour[0])}%`;
      $('hud-pour-foam').style.width = `${Math.min(100, pour[1])}%`;
    }

    const outcome = panels.outcome ?? '';
    $('hud-outcome').hidden = !outcome;
    set('hud-outcome', outcome);

    const track = g.phase4?.track;
    if (track != null && String(track) !== hud.dataset.track) {
      if (hud.dataset.track !== undefined) toast(`♪ Now playing: ${TRACKS[track] ?? 'a song'}`);
      hud.dataset.track = String(track);
    }

    const d = g.drunk;
    const level = d?.level ?? 0;
    set('hud-tier', d?.passedOut ? 'Passed out' : d?.tier ?? 'sober');
    const fill = $('hud-drunk-fill');
    fill.style.width = `${level}%`;
    fill.style.background = DRUNK_COLORS[Math.min(4, Math.floor(level / 25))];
  }, 100);
}
