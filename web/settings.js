// Settings (plan section 10, Phase 7): mouse sensitivity, volume, voice
// mode and graphics preset. Kept in localStorage; Esc (or the gear button)
// opens the panel. The game reads `window.__lcSettings` every frame; voice
// reads it every 100 ms.

const KEY = 'lastcall-settings';
const DEFAULTS = { sensitivity: 1, volume: 1, voice: 'open', graphics: 'high' };

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
  return { ...DEFAULTS, ...saved, graphics };
}

function save(s) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Private mode: settings last for this tab only.
  }
}

window.__lcSettings = loadSettings();

/** Push-to-talk is held (Left Alt) — voice.js reads it. */
window.__lcPushToTalk = false;
addEventListener('keydown', (e) => e.code === 'AltLeft' && (window.__lcPushToTalk = true));
addEventListener('keyup', (e) => e.code === 'AltLeft' && (window.__lcPushToTalk = false));

export function mountSettings() {
  if (document.getElementById('settings')) return;
  const s = window.__lcSettings;
  const panel = document.createElement('form');
  panel.id = 'settings';
  panel.hidden = true;
  panel.onsubmit = () => false;
  panel.innerHTML = `
    <h2>Settings</h2>
    <label>Mouse sensitivity <input name="sensitivity" type="range" min="0.2" max="3" step="0.1"></label>
    <label>Volume <input name="volume" type="range" min="0" max="1" step="0.05"></label>
    <label>Voice
      <select name="voice">
        <option value="open">Open mic</option>
        <option value="push">Push to talk (Left Alt)</option>
        <option value="off">Off</option>
      </select>
    </label>
    <label>Graphics
      <select name="graphics">
        <option value="high">High</option>
        <option value="low">Low (no shadows, no bloom)</option>
      </select>
    </label>
    <button type="button" id="settings-close">Back to the bar</button>`;
  for (const k of Object.keys(DEFAULTS)) panel.elements[k].value = s[k];
  panel.addEventListener('input', () => {
    const next = {
      sensitivity: Number(panel.elements.sensitivity.value),
      volume: Number(panel.elements.volume.value),
      voice: panel.elements.voice.value,
      graphics: panel.elements.graphics.value,
    };
    window.__lcSettings = next;
    save(next);
  });
  const gear = Object.assign(document.createElement('button'), { id: 'settings-open', textContent: '⚙', title: 'Settings (Esc)' });
  const toggle = (open = panel.hidden) => {
    panel.hidden = !open;
    if (open && document.pointerLockElement) document.exitPointerLock();
  };
  gear.addEventListener('click', () => toggle());
  panel.querySelector('#settings-close').addEventListener('click', () => toggle(false));
  addEventListener('keydown', (e) => e.code === 'Escape' && toggle());
  document.body.append(panel, gear);
}
