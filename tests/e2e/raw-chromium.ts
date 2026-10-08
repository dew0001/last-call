// A plain headed Chromium driven over the DevTools protocol, without
// Playwright. Playwright keeps every page "visible" (it emulates focus and
// turns off background throttling), so a test that needs a truly hidden tab
// with real background throttling uses this instead. Needs a display
// (scripts/e2e.sh runs the suite under xvfb-run).
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chromium } from '@playwright/test';

type Target = { id: string; type: string; webSocketDebuggerUrl: string };

export class RawTab {
  private ws: WebSocket;
  private nextId = 0;
  private pending = new Map<number, (v: any) => void>();

  private constructor(ws: WebSocket) {
    this.ws = ws;
    ws.onmessage = (e) => {
      const m = JSON.parse(String(e.data));
      if (m.id && this.pending.has(m.id)) {
        this.pending.get(m.id)!(m.result ?? m.error);
        this.pending.delete(m.id);
      }
    };
  }

  static async attach(url: string): Promise<RawTab> {
    const ws = new WebSocket(url);
    await new Promise((resolve, reject) => {
      ws.onopen = resolve;
      ws.onerror = reject;
    });
    return new RawTab(ws);
  }

  call(method: string, params: object = {}): Promise<any> {
    const id = ++this.nextId;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  /** Evaluate an expression in the page and return its value (JSON-able). */
  async eval<T = unknown>(expression: string): Promise<T> {
    const r = await this.call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    return r?.result?.value as T;
  }

  close() {
    this.ws.close();
  }
}

export class RawChromium {
  private constructor(
    private proc: ChildProcess,
    private port: number,
    private profile: string,
  ) {}

  static async launch(port = 9300 + Math.floor(Math.random() * 500)): Promise<RawChromium> {
    const profile = mkdtempSync(join(tmpdir(), 'lastcall-raw-'));
    const proc = spawn(
      chromium.executablePath(),
      [
        `--remote-debugging-port=${port}`,
        `--user-data-dir=${profile}`,
        '--no-first-run',
        '--no-default-browser-check',
        '--no-sandbox',
        '--use-angle=swiftshader',
        '--enable-unsafe-swiftshader',
        '--autoplay-policy=no-user-gesture-required',
        '--window-size=320,240',
        'about:blank',
      ],
      { stdio: 'ignore' },
    );
    const raw = new RawChromium(proc, port, profile);
    for (let i = 0; i < 100; i++) {
      try {
        await raw.targets();
        return raw;
      } catch {
        await new Promise((r) => setTimeout(r, 200));
      }
    }
    raw.close();
    throw new Error('raw Chromium did not start (is DISPLAY set? run via scripts/e2e.sh)');
  }

  async targets(): Promise<Target[]> {
    return (await fetch(`http://127.0.0.1:${this.port}/json/list`)).json();
  }

  /** The first tab, navigated to `url`. */
  async firstTab(url: string): Promise<{ id: string; tab: RawTab }> {
    const t = (await this.targets()).find((x) => x.type === 'page')!;
    const tab = await RawTab.attach(t.webSocketDebuggerUrl);
    await tab.call('Page.navigate', { url });
    return { id: t.id, tab };
  }

  /** Open a new tab in the same window. */
  async newTab(url: string): Promise<{ id: string; tab: RawTab }> {
    const t: Target = await (
      await fetch(`http://127.0.0.1:${this.port}/json/new?${encodeURI(url)}`, { method: 'PUT' })
    ).json();
    return { id: t.id, tab: await RawTab.attach(t.webSocketDebuggerUrl) };
  }

  /** Bring a tab to the front; the previous front tab becomes hidden. */
  async activate(id: string) {
    await fetch(`http://127.0.0.1:${this.port}/json/activate/${id}`);
  }

  close() {
    this.proc.kill();
    try {
      rmSync(this.profile, { recursive: true, force: true });
    } catch {}
  }
}
