// The pause menu and settings (plan section 10, Phase 7).
//
// In a room, clicking the game locks the mouse to it: the mouse turns the
// view and a crosshair marks the centre. Esc frees the mouse and opens the
// pause menu: Resume, Open the bar (host, during Setup), Settings, Save game
// (host), Quit to title. Settings has three tabs: Graphics, Audio and
// Controls (mouse and rebindable keys). Everything is kept in localStorage.
// The game reads `window.__lcSettings` every frame and `window.__lcLocked`
// to know whether the mouse looks around; voice reads the audio settings.

const KEY = 'lastcall-settings';

/** The rebindable actions, in menu order: [name, label, default key code]. */
export const ACTIONS = [
  ['forward', 'Move forward', 'KeyW'],
  ['back', 'Move back', 'KeyS'],
  ['left', 'Move left', 'KeyA'],
  ['right', 'Move right', 'KeyD'],
  ['sprint', 'Sprint', 'ShiftLeft'],
  ['jump', 'Jump', 'Space'],
  ['crouch', 'Crouch', 'ControlLeft'],
  ['interact', 'Interact / pick up / pour', 'KeyE'],
  ['throw', 'Throw (hold to charge)', 'KeyF'],
  ['drop', 'Drop', 'KeyQ'],
  ['use', 'Drink / eat / use', 'KeyR'],
  ['openBar', 'Open the bar (host)', 'KeyO'],
  ['talk', 'Push to talk', 'AltLeft'],
];
const DEFAULT_KEYS = Object.fromEntries(ACTIONS.map(([a, , k]) => [a, k]));
const DEFAULTS = { sensitivity: 1, invertY: false, volume: 1, voice: 'open', graphics: 'high', keys: DEFAULT_KEYS };

/** A software rasterizer (no GPU): SwiftShader, llvmpipe and the like. */
function softwareRenderer() {
  try {
    const gl = document.createElement('canvas').getContext('webgl2');
    const info = gl?.getExtension('WEBGL_debug_renderer_info');
    const name = String(info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl?.getParameter(gl.RENDERER) ?? '');
    gl?.getExtension('WEBGL_lose_context')?.loseContext();
    return /swiftshader|llvmpipe|software|softpipe/i.test(name);
  } catch {
    return false;
  }
}

export function loadSettings() {
  let saved = {};
  try {
    saved = JSON.parse(localStorage.getItem(KEY) ?? '{}');
  } catch {
    // Private mode or junk: defaults.
  }
  // With no GPU, bloom and shadows cost a quarter second a frame: default to Low.
  const graphics = saved.graphics ?? (softwareRenderer() ? 'low' : DEFAULTS.graphics);
  return { ...DEFAULTS, ...saved, graphics, keys: { ...DEFAULT_KEYS, ...(saved.keys ?? {}) } };
}

function save(s) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Private mode: settings last for this tab only.
  }
}

window.__lcSettings = loadSettings();
window.__lcLocked = false;

/** Push-to-talk is held — voice.js reads it. */
window.__lcPushToTalk = false;
addEventListener('keydown', (e) => e.code === window.__lcSettings.keys.talk && (window.__lcPushToTalk = true));
addEventListener('keyup', (e) => e.code === window.__lcSettings.keys.talk && (window.__lcPushToTalk = false));

/** A key code as a short label: `KeyE` → `E`, `ShiftLeft` → `Left Shift`. */
export function keyLabel(code) {
  if (!code) return '—';
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  const m = code.match(/^(Shift|Control|Alt|Meta)(Left|Right)$/);
  if (m) return `${m[2]} ${m[1] === 'Control' ? 'Ctrl' : m[1]}`;
  return { Space: 'Space', Backquote: '`', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→' }[code] ?? code;
}

const inRoom = () => !!window.__lastCall?.playerId;
const isHost = () => !!window.__lcHost;
const canvas = () => document.getElementById('bevy');

export function toast(text, ms = 2500) {
  const t = Object.assign(document.createElement('div'), { className: 'toast', textContent: text });
  document.body.append(t);
  setTimeout(() => t.remove(), ms);
}

export function mountSettings() {
  if (document.getElementById('menu')) return;
  const s = window.__lcSettings;

  // ---------- the menu ----------
  const menu = document.createElement('div');
  menu.id = 'menu';
  menu.hidden = true;
  menu.innerHTML = `
    <div class="menu-card">
      <section id="menu-main">
        <h2>Paused</h2>
        <button type="button" data-act="resume" class="primary">Resume</button>
        <button type="button" data-act="start" hidden>Open the bar</button>
        <button type="button" data-act="settings">Settings</button>
        <button type="button" data-act="save" hidden>Save game</button>
        <button type="button" data-act="quit">Quit to title</button>
        <p class="menu-note" id="menu-note"></p>
      </section>
      <form id="settings" hidden>
        <h2>Settings</h2>
        <nav class="tabs">
          <button type="button" data-tab="graphics" class="on">Graphics</button>
          <button type="button" data-tab="audio">Audio</button>
          <button type="button" data-tab="controls">Controls</button>
        </nav>
        <div data-pane="graphics">
          <label>Quality
            <select name="graphics">
              <option value="high">High (shadows, bloom)</option>
              <option value="low">Low (faster)</option>
            </select>
          </label>
        </div>
        <div data-pane="audio" hidden>
          <label>Volume <input name="volume" type="range" min="0" max="1" step="0.05"></label>
          <label>Voice chat
            <select name="voice">
              <option value="open">Open mic</option>
              <option value="push">Push to talk</option>
              <option value="off">Off</option>
            </select>
          </label>
        </div>
        <div data-pane="controls" hidden>
          <label>Mouse sensitivity <input name="sensitivity" type="range" min="0.2" max="3" step="0.1"></label>
          <label>Invert mouse Y <input name="invertY" type="checkbox"></label>
          <table class="keys"><tbody></tbody></table>
          <button type="button" data-act="reset-keys">Reset keys to defaults</button>
        </div>
        <button type="button" data-act="back" class="primary">Back</button>
      </form>
    </div>`;
  document.body.append(menu);
  const main = menu.querySelector('#menu-main');
  const form = menu.querySelector('#settings');
  const btn = (act) => menu.querySelector(`[data-act="${act}"]`);

  // ---------- settings form ----------
  form.onsubmit = () => false;
  form.elements.graphics.value = s.graphics;
  form.elements.volume.value = s.volume;
  form.elements.voice.value = s.voice;
  form.elements.sensitivity.value = s.sensitivity;
  form.elements.invertY.checked = !!s.invertY;
  const update = (patch) => {
    window.__lcSettings = { ...window.__lcSettings, ...patch };
    save(window.__lcSettings);
  };
  form.addEventListener('input', (e) => {
    if (!e.target.name) return;
    update({
      graphics: form.elements.graphics.value,
      volume: Number(form.elements.volume.value),
      voice: form.elements.voice.value,
      sensitivity: Number(form.elements.sensitivity.value),
      invertY: form.elements.invertY.checked,
    });
  });
  for (const tab of form.querySelectorAll('[data-tab]')) {
    tab.addEventListener('click', () => {
      for (const t of form.querySelectorAll('[data-tab]')) t.classList.toggle('on', t === tab);
      for (const p of form.querySelectorAll('[data-pane]')) p.hidden = p.dataset.pane !== tab.dataset.tab;
    });
  }

  // Key rebinding: click a key, then press the new one (Esc cancels).
  const tbody = form.querySelector('.keys tbody');
  let listening = null;
  const drawKeys = () => {
    tbody.replaceChildren(
      ...ACTIONS.map(([action, label]) => {
        const tr = document.createElement('tr');
        const b = Object.assign(document.createElement('button'), {
          type: 'button',
          className: 'keycap',
          textContent: listening === action ? 'Press a key…' : keyLabel(window.__lcSettings.keys[action]),
        });
        b.dataset.bind = action;
        b.addEventListener('click', () => {
          listening = action;
          drawKeys();
        });
        const td = document.createElement('td');
        td.append(b);
        tr.append(Object.assign(document.createElement('td'), { textContent: label }), td);
        return tr;
      }),
    );
  };
  drawKeys();
  addEventListener(
    'keydown',
    (e) => {
      if (!listening) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.code !== 'Escape') {
        const keys = { ...window.__lcSettings.keys };
        // A key does one thing: whatever had it before is left unbound.
        for (const [a, k] of Object.entries(keys)) if (k === e.code) keys[a] = '';
        keys[listening] = e.code;
        update({ keys });
      }
      listening = null;
      drawKeys();
    },
    true,
  );
  btn('reset-keys').addEventListener('click', () => {
    update({ keys: { ...DEFAULT_KEYS } });
    drawKeys();
  });

  // ---------- open, close, actions ----------
  const showMain = () => {
    form.hidden = true;
    main.hidden = false;
  };
  const refresh = () => {
    const shift = window.__lastCall?.game?.shift;
    btn('resume').hidden = !inRoom();
    btn('save').hidden = !(inRoom() && isHost());
    btn('quit').hidden = !inRoom();
    btn('start').hidden = !(isHost() && shift?.waiting);
    main.querySelector('h2').textContent = inRoom() ? 'Paused' : 'Menu';
    menu.querySelector('#menu-note').textContent =
      inRoom() && isHost() ? 'You are the host: the room closes if you quit.' : '';
  };
  const open = () => {
    refresh();
    showMain();
    menu.hidden = false;
    if (document.pointerLockElement) document.exitPointerLock();
  };
  const close = () => {
    menu.hidden = true;
    listening = null;
  };
  const resume = () => {
    close();
    // Chrome refuses a new lock for about a second after Esc: then a click
    // on the game locks it.
    Promise.resolve(canvas()?.requestPointerLock?.()).catch(() => toast('Click the game to look around.'));
  };
  window.__lcMenu = { open, close, isOpen: () => !menu.hidden };

  btn('resume').addEventListener('click', resume);
  btn('settings').addEventListener('click', () => {
    main.hidden = true;
    form.hidden = false;
  });
  btn('back').addEventListener('click', showMain);
  btn('start').addEventListener('click', () => {
    window.__lcHost?.startShift();
    resume();
  });
  btn('save').addEventListener('click', () => {
    window.__lcHost?.save();
    menu.querySelector('#menu-note').textContent = 'Saving…';
  });
  addEventListener('lastcall-saved', () => {
    toast('Game saved. Continue it from the title screen.');
    if (!menu.hidden) menu.querySelector('#menu-note').textContent = 'Saved.';
  });
  btn('quit').addEventListener('click', () => {
    const warn = isHost()
      ? 'Quit to the title screen? The room closes for everyone. Progress since your last save is lost.'
      : 'Leave the room and go back to the title screen?';
    if (confirm(warn)) location.href = location.pathname;
  });

  // The gear (title screen and in game) opens the menu.
  const gear = Object.assign(document.createElement('button'), { id: 'settings-open', textContent: '⚙', title: 'Menu (Esc)' });
  gear.addEventListener('click', () => (menu.hidden ? open() : close()));
  document.body.append(gear);

  // Esc: the browser frees a locked mouse itself (and the menu opens below);
  // with a free mouse, Esc opens or closes the menu.
  addEventListener('keydown', (e) => {
    if (e.code !== 'Escape' || listening) return;
    if (menu.hidden) open();
    else if (!form.hidden) showMain();
    else close();
  });

  // ---------- mouse lock and crosshair ----------
  const cross = Object.assign(document.createElement('div'), { id: 'crosshair' });
  cross.hidden = true;
  document.body.append(cross);
  document.addEventListener('pointerlockchange', () => {
    const locked = !!document.pointerLockElement;
    window.__lcLocked = locked;
    cross.hidden = !locked;
    document.body.classList.toggle('locked', locked);
    // Losing the lock in a room (Esc, alt-tab) pauses, like any shooter.
    if (!locked && inRoom() && menu.hidden) open();
  });

  // ---------- the host opens the bar ----------
  const prompt = Object.assign(document.createElement('div'), { id: 'shift-prompt' });
  prompt.hidden = true;
  document.body.append(prompt);
  setInterval(() => {
    const shift = window.__lastCall?.game?.shift;
    const waiting = inRoom() && !!shift?.waiting;
    prompt.hidden = !waiting;
    if (waiting) {
      const key = keyLabel(window.__lcSettings.keys.openBar);
      prompt.textContent = isHost()
        ? `Setup: get the bar ready. Press ${key} to open the doors (or Esc → Open the bar).`
        : 'Setup: waiting for the host to open the bar.';
    }
    if (!menu.hidden) refresh();
  }, 250);
  addEventListener('keydown', (e) => {
    if (e.code !== window.__lcSettings.keys.openBar || !isHost() || !menu.hidden) return;
    if (window.__lastCall?.game?.shift?.waiting) window.__lcHost.startShift();
  });
}
