// Picks the renderer, loads the matching client bundle, and exposes test hooks.
//
// Bevy's WebGPU build cannot fall back to WebGL2, so there are two bundles.
// `?gpu=webgl2` or `?gpu=webgpu` forces one; otherwise WebGPU is used when the
// browser gives us an adapter.

const params = new URLSearchParams(location.search);

async function webgpuAvailable() {
  try {
    if (!('gpu' in navigator)) return false;
    return (await navigator.gpu.requestAdapter()) !== null;
  } catch {
    return false;
  }
}

const FALLBACK_KEY = 'lastcall.webgpuFailed';

function webgpuFailedBefore() {
  try {
    return sessionStorage.getItem(FALLBACK_KEY) === '1';
  } catch {
    return false;
  }
}

async function pickVariant() {
  const forced = params.get('gpu');
  if (forced === 'webgl2' || forced === 'webgpu') return forced;
  if (webgpuFailedBefore()) return 'webgl2';
  return (await webgpuAvailable()) ? 'webgpu' : 'webgl2';
}

// Some browsers hand out a WebGPU adapter whose device dies right away (for
// example headless Chromium on SwiftShader). When the auto-picked WebGPU build
// stops drawing early, remember that for this tab and reload on WebGL2.
function watchWebgpu() {
  const started = performance.now();
  const timer = setInterval(() => {
    const frames = window.__lastCall?.frames ?? 0;
    if (frames > 30) {
      clearInterval(timer);
      return;
    }
    if (performance.now() - started > 20000) {
      clearInterval(timer);
      try {
        sessionStorage.setItem(FALLBACK_KEY, '1');
      } catch {
        // No storage: reload with an explicit flag instead.
        location.search = '?gpu=webgl2';
        return;
      }
      location.reload();
    }
  }, 500);
}

// Host simulation in a dedicated Web Worker. Tick reports land in window.__hostTicks.
window.__hostTicks = [];
window.startHostWorker = () => {
  const worker = new Worker(new URL('./host-worker.js', import.meta.url), { type: 'module' });
  worker.onmessage = (e) => {
    if (e.data?.type === 'tick') window.__hostTicks.push(e.data);
  };
  worker.onerror = (e) => console.error('host worker error', e.message);
  return worker;
};

async function boot() {
  const variant = await pickVariant();
  window.__lastCallVariant = variant;
  if (variant === 'webgpu' && !params.has('gpu')) watchWebgpu();
  const mod = await import(`./pkg/client_${variant}.js`);
  try {
    await mod.default();
  } catch (e) {
    // winit hands control to the browser event loop by throwing on purpose.
    if (!String(e).includes('Using exceptions for control flow')) throw e;
  }
  document.getElementById('boot')?.remove();
}

if (params.has('hostonly')) {
  // Test page: boot only the host worker, no renderer.
  window.__lastCallVariant = 'none';
  document.getElementById('boot').textContent = 'Host worker only';
  window.startHostWorker();
} else {
  boot().catch((e) => {
    console.error(e);
    const el = document.getElementById('boot');
    if (el) el.textContent = `Failed to start: ${e}`;
    window.__lastCallError = String(e);
  });
}
