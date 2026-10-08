// Phase 1: rooms, WebRTC netcode, movement.
// Needs the signaling Worker under `wrangler dev` (started by playwright.config.ts).
import { expect, test, type Browser, type Page } from '@playwright/test';

test.beforeEach(({ page }, info) => {
  const tag = `[${info.project.name}]`;
  page.on('pageerror', (e) => console.log(`${tag} pageerror: ${e.message.slice(0, 500)}`));
});

type Status = {
  frames: number;
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
async function openTab(browser: Browser, url: string, tag: string) {
  // Small viewports: the cloud and CI have no GPU, and software rendering cost
  // grows with pixels. Many tabs share 4 CPU cores.
  const context = await browser.newContext({ viewport: { width: 320, height: 180 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.log(`[${tag}] pageerror: ${e.message.slice(0, 300)}`));
  page.on('console', (m) => {
    if (m.type() === 'error') console.log(`[${tag}] console.error: ${m.text().slice(0, 300)}`);
  });
  await page.goto(url);
  return page;
}

async function createRoom(browser: Browser) {
  const host = await openTab(browser, '/?create&gpu=webgl2&name=Host', 'host');
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
