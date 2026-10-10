// The in-game HUD, drawn by the page over the canvas from the game status
// (`window.__lastCall`, published by the client 20 times a second):
//   top left:     week and shift, the phase and its clock
//   top right:    the house pool, your pocket, the debt and this week's due
//   bottom left:  the drunk meter
// The pour gauge and the end-of-run screen stay in the game.

const PHASES = { SETUP: 'Setup', OPEN: 'Open', 'LAST CALL': 'Last call', PAYMENT: 'Payment' };
const DRUNK_COLORS = ['#6fd4ff', '#ffd25a', '#ff9a3c', '#ff4f6d', '#c04dff'];

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

    const d = g.drunk;
    const level = d?.level ?? 0;
    set('hud-tier', d?.passedOut ? 'Passed out' : d?.tier ?? 'sober');
    const fill = $('hud-drunk-fill');
    fill.style.width = `${level}%`;
    fill.style.background = DRUNK_COLORS[Math.min(4, Math.floor(level / 25))];
  }, 100);
}
