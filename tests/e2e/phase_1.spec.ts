// Phase 1: rooms, WebRTC netcode, movement.
// Needs the signaling Worker under `wrangler dev` (started by playwright.config.ts).
import { expect, test, type Browser, type Page } from '@playwright/test';

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
  // Small viewports: the cloud and CI have no GPU, and software rendering cost
  // grows with pixels. Many tabs share 4 CPU cores.
  const context = await browser.newContext({ viewport: size });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.log(`[${tag}] pageerror: ${e.message.slice(0, 300)}`));
  page.on('console', (m) => {
    if (m.type() === 'error') console.log(`[${tag}] console.error: ${m.text().slice(0, 300)}`);
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
