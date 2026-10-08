// Phase 1: rooms, WebRTC netcode, movement.
// Needs the signaling Worker under `wrangler dev` (started by playwright.config.ts).
import { expect, test, type Browser, type Page } from '@playwright/test';
import { RawChromium } from './raw-chromium';

test.beforeEach(({ page }, info) => {
  const tag = `[${info.project.name}]`;
  page.on('pageerror', (e) => console.log(`${tag} pageerror: ${e.message.slice(0, 500)}`));
});

type Status = {
  frames: number;
  propsSeen?: number;
  holding?: boolean;
  propsOnFloor?: number;
  connected?: boolean;
  playerId?: string | null;
  playersSeen?: number;
  ownPos?: [number, number, number] | null;
};

const status = (page: Page) =>
  page.evaluate(() => ({ ...((window as any).__lastCall ?? {}), error: (window as any).__lastCallError })) as Promise<
    Status & { error?: string }
  >;

/** Poll a page until `check` passes, failing fast on a client error. */
async function waitFor(page: Page, what: string, check: (s: Status) => boolean, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  let last: Status | undefined;
  while (Date.now() < deadline) {
    const s = await status(page).catch(() => undefined);
    if (s?.error) throw new Error(`${what}: client error: ${s.error.slice(0, 300)}`);
    if (s && check(s)) return s;
    last = s;
    await page.waitForTimeout(250);
  }
  throw new Error(`${what}: timed out; last status ${JSON.stringify(last)}`);
}

/** Each tab gets its own browser context, like a separate player's machine. */
async function openTab(browser: Browser, url: string, tag: string, size = { width: 160, height: 90 }) {
  // These tests check game state, not pixels (phase 0 covers rendering), so
  // tabs run the full client without drawing. Software rendering on a
  // GPU-less CI runner otherwise starves eight tabs of frames.
  url += url.includes('?') ? '&nodraw' : '?nodraw';
  // Small viewports: the cloud and CI have no GPU, and software rendering cost
  // grows with pixels. Many tabs share 4 CPU cores.
  const context = await browser.newContext({ viewport: size });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.log(`[${tag}] pageerror: ${e.message.slice(0, 300)}`));
  page.on('console', (m) => {
    if (m.type() === 'error') console.log(`[${tag}] console.error: ${m.text().slice(0, m.text().startsWith('host worker stack') ? 8000 : 300)}`);
  });
  await page.goto(url);
  return page;
}

async function createRoom(browser: Browser, size?: { width: number; height: number }) {
  const host = await openTab(browser, '/?create&gpu=webgl2&name=Host', 'host', size);
  await expect.poll(() => host.evaluate(() => (window as any).__lcRoom?.code ?? null), { timeout: 60_000 }).toMatch(
    /^[BCDFGHJKLMNPQRSTVWXYZ]{5}$/,
  );
  const room = await host.evaluate(() => (window as any).__lcRoom);
  await waitFor(host, 'host joins its own room', (s) => !!s.playerId && (s.playersSeen ?? 0) >= 1);
  return { host, room: room as { code: string; link: string } };
}

test('Create Room gives a code and a link; a second tab joins over WebRTC', async ({ browser }) => {
  const { host, room } = await createRoom(browser);
  expect(room.link).toContain(`?room=${room.code}`);

  const started = Date.now();
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Guest`, 'player');
  await waitFor(player, 'player sees both players', (s) => !!s.playerId && s.playersSeen === 2);
  const toLobbyMs = Date.now() - started;
  test.info().annotations.push({ type: 'time-to-lobby-ms', description: String(toLobbyMs) });
  expect(toLobbyMs).toBeLessThan(15_000);

  // A covered window stops drawing (Firefox runs headed here). The host sim
  // keeps running in its Worker; bring the tab forward to read its view.
  await host.bringToFront();
  await waitFor(host, 'host sees both players', (s) => s.playersSeen === 2);
  await player.context().close();
  await host.context().close();
});

test('a player moves with input and the prediction follows', async ({ browser }) => {
  const { host, room } = await createRoom(browser);
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Walker`, 'player');
  const s0 = await waitFor(player, 'player spawned', (s) => !!s.ownPos);
  // Walk forward (-Z) for two seconds.
  await player.evaluate(() => ((window as any).__lcInput = { mx: 0, my: 1, yaw: 0, pitch: 0, buttons: 0 }));
  await player.waitForTimeout(2000);
  await player.evaluate(() => ((window as any).__lcInput = { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: 0 }));
  const s1 = await waitFor(player, 'player moved', (s) => !!s.ownPos && s.ownPos[2] < s0.ownPos![2] - 3);
  test.info().annotations.push({ type: 'moved', description: `${s0.ownPos} -> ${s1.ownPos}` });
  await player.context().close();
  await host.context().close();
});

type Input = { mx: number; my: number; yaw: number; pitch: number; buttons: number };
const setInput = (page: Page, input: Input) => page.evaluate((i) => ((window as any).__lcInput = i), input);
const INTERACT = 1 << 3;
const THROW = 1 << 4;

test('a player picks up a bottle and throws it; the host sees it land', async ({ browser }) => {
  const { host, room } = await createRoom(browser);
  // Slot 1 spawns at x = -5, in front of bottles on the counter.
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Thrower`, 'player');
  await waitFor(player, 'player sees all props', (s) => s.propsSeen === 130 && !!s.ownPos);
  const before = (await waitFor(host, 'host sees all props', (s) => s.propsSeen === 130)).propsOnFloor ?? 0;

  // Walk into the counter, grab, turn around, charge and throw.
  await setInput(player, { mx: 0, my: 1, yaw: 0, pitch: 0, buttons: 0 });
  await waitFor(player, 'player reached the counter', (s) => !!s.ownPos && s.ownPos[2] < -3.0, 20_000);
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: INTERACT });
  await waitFor(player, 'player holds a prop', (s) => !!s.holding, 10_000);
  await setInput(player, { mx: 0, my: 0, yaw: Math.PI, pitch: 0, buttons: 0 });
  await player.waitForTimeout(800);
  await setInput(player, { mx: 0, my: 0, yaw: Math.PI, pitch: 0, buttons: THROW });
  await player.waitForTimeout(500);
  await setInput(player, { mx: 0, my: 0, yaw: Math.PI, pitch: 0, buttons: 0 });
  await waitFor(player, 'player let go', (s) => !s.holding, 10_000);

  await host.bringToFront();
  await waitFor(host, 'host sees the bottle on the floor', (s) => (s.propsOnFloor ?? 0) > before, 20_000);
  await player.context().close();
  await host.context().close();
});

test('8 clients (host plus 7) walk around the gray-box bar', async ({ browser, browserName }) => {
  test.skip(
    browserName === 'firefox',
    'headed Firefox runs only the front window; 8 windows on one virtual display cannot all draw (docs/DECISIONS.md)',
  );
  test.setTimeout(300_000);
  // Eight renderers share 4 cores with no GPU: keep them tiny.
  const tiny = { width: 160, height: 90 };
  const { host, room } = await createRoom(browser, tiny);
  const tabs: Page[] = [host];
  // Open tabs one at a time and let each join before the next. Headed Firefox
  // only runs the frame loop of the window in front, and a page that never
  // draws a frame never joins.
  for (let i = 1; i < 8; i++) {
    const tab = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Bot${i}`, `bot${i}`, tiny);
    await waitFor(tab, `tab ${i} joined`, (s) => !!s.playerId && !!s.ownPos, 60_000);
    tabs.push(tab);
  }
  for (const [i, tab] of tabs.entries()) {
    await tab.bringToFront();
    await waitFor(tab, `tab ${i} sees 8 players`, (s) => s.playersSeen === 8 && !!s.ownPos, 120_000);
  }
  // Everyone walks in a different direction. Each tab walks while in front.
  const starts: [number, number, number][] = [];
  for (const [i, tab] of tabs.entries()) {
    await tab.bringToFront();
    starts.push((await status(tab)).ownPos!);
    await setInput(tab, { mx: 0, my: 1, yaw: (i * Math.PI) / 4, pitch: 0, buttons: 0 });
    const s = await waitFor(tab, `tab ${i} moved`, (s) => {
      const p = s.ownPos!;
      return Math.hypot(p[0] - starts[i][0], p[2] - starts[i][2]) > 1.0;
    });
    test.info().annotations.push({ type: `tab-${i}`, description: `${starts[i]} -> ${s.ownPos}` });
  }
  // Host tick cost with 8 players (budget: 6 ms per tick).
  const reports: { tickAvgMs: number; tickMaxMs: number }[] = await host.evaluate(() =>
    (window as any).__hostTicks.slice(-5),
  );
  const avg = Math.max(...reports.map((r) => r.tickAvgMs));
  const max = Math.max(...reports.map((r) => r.tickMaxMs));
  test.info().annotations.push({ type: 'host-tick-ms', description: `avg ${avg.toFixed(2)} max ${max.toFixed(2)}` });
  // The budget is checked in Chromium, the performance reference. In WebKit,
  // eight browser processes share the runner's 4 cores and the wall-clock
  // tick time mostly measures that contention; it is only recorded
  // (docs/DECISIONS.md).
  if (browserName === 'chromium') expect(avg).toBeLessThan(6);
  for (const tab of tabs) await tab.context().close();
});

test('voice: a remote voice gets quieter as its speaker walks away', async ({ browser, browserName }) => {
  test.skip(browserName === 'webkit', 'Playwright WebKit has no fake microphone; see docs/DECISIONS.md');
  const { host, room } = await createRoom(browser);
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Talker`, 'player');
  const ps = await waitFor(player, 'player joined', (s) => !!s.playerId && !!s.ownPos);
  // Browsers start audio only after a user gesture; players click into the game.
  await player.mouse.click(80, 45);
  await host.bringToFront();
  await host.mouse.click(80, 45);

  // The host hears the player through a level meter after the distance gain.
  await expect
    .poll(() => host.evaluate((id) => (window as any).__lcVoice?.peers().includes(id) ?? false, ps.playerId), {
      timeout: 30_000,
    })
    .toBe(true);
  // Peak level over a window (the fake microphone beeps).
  const peak = (ms: number) =>
    host.evaluate(
      async ([id, ms]) => {
        const v = (window as any).__lcVoice;
        let max = 0;
        const end = performance.now() + (ms as number);
        while (performance.now() < end) {
          max = Math.max(max, v.level(id));
          await new Promise((r) => setTimeout(r, 20));
        }
        return max;
      },
      [ps.playerId, ms] as const,
    );
  await expect.poll(() => peak(1500), { timeout: 30_000 }).toBeGreaterThan(0.01);
  const near = await peak(2000);

  // The player walks along +X, from 2 m to more than 14 m away.
  await setInput(player, { mx: 0, my: 1, yaw: -Math.PI / 2, pitch: 0, buttons: 0 });
  await waitFor(player, 'player walked away', (s) => !!s.ownPos && s.ownPos[0] > 7.5, 20_000);
  await setInput(player, { mx: 0, my: 0, yaw: -Math.PI / 2, pitch: 0, buttons: 0 });
  await host.bringToFront();
  await host.waitForTimeout(1500);
  const far = await peak(2000);
  test.info().annotations.push({ type: 'voice-level', description: `near ${near.toFixed(4)} far ${far.toFixed(4)}` });
  expect(far).toBeLessThan(near * 0.2);
  await player.context().close();
  await host.context().close();
});

test('refreshing a player tab rejoins the same player', async ({ browser }) => {
  const { host, room } = await createRoom(browser);
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Refresher`, 'player');
  const before = await waitFor(player, 'player joined', (s) => !!s.playerId && !!s.ownPos);
  await setInput(player, { mx: 0, my: 1, yaw: 0, pitch: 0, buttons: 0 });
  await waitFor(player, 'player walked', (s) => !!s.ownPos && s.ownPos[2] < 1.5, 20_000);
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: 0 });
  await player.waitForTimeout(1000);
  const walked = (await status(player)).ownPos!;

  await player.reload();
  const after = await waitFor(player, 'player rejoined', (s) => !!s.playerId && !!s.ownPos && s.playersSeen === 2);
  expect(after.playerId).toBe(before.playerId);
  const moved = Math.hypot(after.ownPos![0] - walked[0], after.ownPos![2] - walked[2]);
  test.info().annotations.push({ type: 'rejoin', description: `${walked} -> ${after.ownPos} (${moved.toFixed(2)} m)` });
  expect(moved).toBeLessThan(1.0);
  await host.bringToFront();
  await waitFor(host, 'host still sees two players', (s) => s.playersSeen === 2);
  await player.context().close();
  await host.context().close();
});

test('closing the host tab shows "Host left" on every client within 5 s', async ({ browser }) => {
  const { host, room } = await createRoom(browser);
  const players: Page[] = [];
  for (let i = 0; i < 2; i++) {
    const p = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Stayer${i}`, `player${i}`);
    await waitFor(p, `player ${i} joined`, (s) => !!s.playerId);
    players.push(p);
  }
  const closedAt = Date.now();
  await host.context().close();
  for (const [i, p] of players.entries()) {
    await expect
      .poll(() => p.evaluate(() => (window as any).__lcHostLeft ?? null), { timeout: 5_000, intervals: [100] })
      .toBe('host left');
    test.info().annotations.push({ type: `host-left-ms-${i}`, description: String(Date.now() - closedAt) });
    await expect(p.locator('#banner')).toContainText('Host left');
  }
  expect(Date.now() - closedAt).toBeLessThan(5_000);
  for (const p of players) await p.context().close();
});

test('a hidden host tab keeps a 64 Hz tick for 60 s', async ({ browserName }, info) => {
  // Playwright keeps every page "visible", so this test drives a plain headed
  // Chromium where a background tab is really hidden and throttled.
  test.skip(browserName !== 'chromium', 'drives Chromium directly over the DevTools protocol');
  test.setTimeout(180_000);
  const base = String(info.project.use.baseURL ?? 'http://localhost:8080');
  const raw = await RawChromium.launch();
  try {
    const { tab: host } = await raw.firstTab(`${base}/?create&gpu=webgl2&novoice&nodraw&name=Host`);
    let link: string | undefined;
    for (let i = 0; i < 120 && !link; i++) {
      await new Promise((r) => setTimeout(r, 500));
      link = await host.eval<string | undefined>('window.__lcRoom?.link');
    }
    expect(link).toBeTruthy();
    // The player tab shares the profile, so give it its own identity.
    const { id: playerId, tab: player } = await raw.newTab(
      `${link}&gpu=webgl2&novoice&nodraw&name=Witness&player=00000000-0000-4000-8000-00000000beef`,
    );
    await raw.activate(playerId);
    let me: string | null = null;
    for (let i = 0; i < 120 && !me; i++) {
      await new Promise((r) => setTimeout(r, 500));
      me = await player.eval<string | null>('window.__lastCall?.playerId ?? null');
    }
    expect(me).toBeTruthy();
    expect(await host.eval('document.visibilityState')).toBe('hidden');

    // The player walks while the host tab is hidden; the host sim moves it.
    await player.eval('window.__lcInput = { mx: 0, my: 1, yaw: Math.PI, pitch: 0, buttons: 0 }');
    const start = await host.eval<number>('window.__hostTicks.length');
    await new Promise((r) => setTimeout(r, 60_000));
    const reports = await host.eval<{ tps: number; players: [string, number, number, number][] }[]>(
      `window.__hostTicks.slice(${start})`,
    );
    expect(await host.eval('document.visibilityState')).toBe('hidden');
    const rates = reports.map((r) => r.tps);
    info.annotations.push({
      type: 'hidden-tps',
      description: `min ${Math.min(...rates).toFixed(1)} over ${rates.length} reports, tab hidden`,
    });
    expect(rates.length).toBeGreaterThanOrEqual(55);
    for (const tps of rates.slice(1)) expect(tps).toBeGreaterThan(62);
    const track = reports.map((r) => r.players.find((p) => p[0] === me)).filter(Boolean) as [string, number, number, number][];
    expect(track.length).toBeGreaterThan(10);
    expect(track[track.length - 1][3]).not.toBe(track[0][3]);
    host.close();
    player.close();
  } finally {
    raw.close();
  }
});
