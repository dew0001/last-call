// The in-game HUD, drawn by the page over the canvas from the game status
// (`window.__lastCall`, published by the client 20 times a second):
//   top left:     week and shift, the phase and its clock
//   top right:    the house pool, your pocket, the debt and this week's due
//   bottom left:  the drunk meter
//   bottom right: the table you stand at (blackjack, roulette, slots)
// The pour gauge and the end-of-run screen stay in the game.

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
  const m = head.match(/^(\S+)\s*(.*)$/) ?? [head, head, ''];
  const caps = (line) =>
    esc(line).replace(/(^|\s)(Enter|[A-Z0-9]|[A-Z]\/[A-Z])(?=\s|$)/g, (_, pre, k) => `${pre}<kbd>${k}</kbd>`);
  return (
    `<div class="hud-table-title"><span>${esc(m[1])}</span><small>${esc(m[2])}</small></div>` +
    body.filter((l) => l.trim()).map((l) => `<div class="hud-table-row">${esc(l)}</div>`).join('') +
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
    <div class="hud-card hud-table" id="hud-table" hidden></div>
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

    const panel = s.tablePanel ?? '';
    const table = $('hud-table');
    table.hidden = !panel;
    if (panel && table.dataset.text !== panel) {
      table.dataset.text = panel;
      table.innerHTML = tableCard(panel);
    }

    const d = g.drunk;
    const level = d?.level ?? 0;
    set('hud-tier', d?.passedOut ? 'Passed out' : d?.tier ?? 'sober');
    const fill = $('hud-drunk-fill');
    fill.style.width = `${level}%`;
    fill.style.background = DRUNK_COLORS[Math.min(4, Math.floor(level / 25))];
  }, 100);
}
