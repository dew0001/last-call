// Phase 2: shift loop and economy.
// Rooms run with `&fast=60`: every shift phase is 60 times shorter, so a
// 14-minute shift takes 14 seconds (Setup 2, Open 9, Last call 2, Payment 1).
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { DROP, INTERACT, ROUTE_TO_SAFE, ROUTE_TO_TAP, createRoom, openTab, setInput, status, tap, waitFor, walkTo } from './helpers';

test.beforeEach(({ page }, info) => {
  const tag = `[${info.project.name}]`;
  page.on('pageerror', (e) => console.log(`${tag} pageerror: ${e.message.slice(0, 500)}`));
});

test('the shift clock runs Setup, Open, Last call, Payment and rolls into the next shift', async ({ browser }) => {
  const { host, room } = await createRoom(browser, undefined, '&fast=60');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && !!s.game?.shift);

  // Record every (shift, phase) the player sees until shift 2 starts.
  const seen: string[] = [];
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const s = await status(player);
    const c = s.game?.shift;
    if (c) {
      expect(c.running).toBe(true);
      const key = `${c.week}.${c.shift} ${c.phase}`;
      if (seen.at(-1) !== key) seen.push(key);
      if (c.shift === 2 && c.phase === 'OPEN') break;
    }
    await player.waitForTimeout(150);
  }
  test.info().annotations.push({ type: 'phases', description: seen.join(' > ') });
  const from = seen.indexOf('1.1 OPEN');
  expect(from).toBeGreaterThanOrEqual(0);
  expect(seen.slice(from, from + 5)).toEqual(['1.1 OPEN', '1.1 LAST CALL', '1.1 PAYMENT', '1.2 SETUP', '1.2 OPEN']);

  // The host's own client shows the same clock.
  const h = (await status(host)).game?.shift;
  expect([h?.week, h?.shift]).toEqual([1, 2]);
  await player.context().close();
  await host.context().close();
});

test('money goes from a pocket into the house pool at the office safe', async ({ browser }) => {
  // Normal speed, so the walk has all the time it needs. 300 in each pocket.
  const { host, room } = await createRoom(browser, undefined, '&preset=lastweek');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  const s0 = await waitFor(player, 'player joined', (s) => !!s.playerId && s.game?.pocket === 300 && !!s.game?.money);
  expect(s0.game.money.house).toBe(45_000);

  // Walk into the office and press E at the safe three times.
  await walkTo(player, ROUTE_TO_SAFE);
  for (let i = 0; i < 3; i++) await tap(player, INTERACT);
  const s1 = await waitFor(player, 'pocket emptied into the house', (s) => s.game?.pocket === 0 && s.game.money.house > 45_000);
  expect(s1.game.money.house).toBe(45_300);
  await player.context().close();
  await host.context().close();
});

test('the last payment wins, and a new run starts', async ({ browser }) => {
  // Week 6, 80,000 paid, 45,000 in the house.
  const { host, room } = await createRoom(browser, undefined, '&fast=60&preset=lastweek');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  const s0 = await waitFor(player, 'player joined', (s) => !!s.playerId && !!s.game?.money);
  expect(s0.game.money.due).toBe(40_000);

  // At the end of week 6 the loan shark takes the 40,000 left: the run is won.
  // Both clients show the win screen (only 1 second long at fast=60).
  const [won] = await Promise.all([
    waitFor(player, 'run won', (s) => s.game?.money?.outcome === 'won', 90_000),
    waitFor(host, 'host sees the win', (s) => s.game?.money?.outcome === 'won', 90_000),
  ]);
  expect(won.game.money.paid).toBe(120_000);
  expect(won.game.money.last).toEqual(['paid', 40_000]);

  // Then a new run at new game plus 1, with no money carried over.
  // The ledger and the clock reset in consecutive ticks; wait for both.
  const next = await waitFor(
    player,
    'new run',
    (s) => s.game?.money?.outcome === 'playing' && s.game.money.ng === 1 && s.game.shift?.week === 1,
  );
  expect(next.game.money.debt).toBe(150_000);
  expect(next.game.money.house).toBe(0);
  expect(next.game.pocket).toBe(0);
  expect(next.game.shift.week).toBe(1);
  await player.context().close();
  await host.context().close();
});

test('a second missed payment burns the bar down', async ({ browser }) => {
  // One payment already missed, nothing in the house.
  const { host, room } = await createRoom(browser, undefined, '&fast=60&preset=broke');
  const lost = await waitFor(host, 'run lost', (s) => s.game?.money?.outcome === 'lost', 90_000);
  expect(lost.game.money.missedInARow).toBe(2);
  expect(lost.game.money.last).toEqual(['missed', 16_000]);
  await host.context().close();
});

test('customers walk in, sit at the bar, order, and leave at last call', async ({ browser }) => {
  // fast=20: Setup 6 s, Open 27 s with a wave every 4.5 s, Last call 6 s.
  const { host, room } = await createRoom(browser, undefined, '&fast=20&customers=bar');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && !!s.game?.shift);

  // A customer reaches a stool in front of the counter and waits for a beer.
  type C = [number, string, number, number];
  const seated = await waitFor(
    player,
    'a customer waits at the bar',
    (s) => (s.game?.customers ?? []).some((c: C) => c[1] === 'waiting' && c[3] > -3.3 && c[3] < -2.0),
    60_000,
  );
  const n = seated.game.customers.length;
  test.info().annotations.push({ type: 'customers', description: JSON.stringify(seated.game.customers) });
  expect(n).toBeGreaterThanOrEqual(6);

  // Last call: everyone walks out, and nobody is left by the next Setup.
  await waitFor(player, 'last call', (s) => s.game?.shift?.phase === 'LAST CALL', 60_000);
  await waitFor(player, 'everyone leaving', (s) => (s.game?.customers ?? []).every((c: C) => c[1] === 'leaving'), 5_000);
  await waitFor(player, 'bar empty by next setup', (s) => s.game?.shift?.shift === 2 && s.game.customers.length === 0, 60_000);
  await player.context().close();
  await host.context().close();
});

test('a perfect pour, carried to a waiting customer, is paid for and tipped', async ({ browser }) => {
  // fast=20: Setup 6 s, then customers arrive at the start of Open.
  const { host, room } = await createRoom(browser, undefined, '&fast=20&customers=bar');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && !!s.game?.money);

  // Stand at the tap and wait for a customer to sit down.
  await walkTo(player, ROUTE_TO_TAP);
  type C = [number, string, number, number];
  const waiting = (s: any) => (s.game?.customers ?? []).filter((c: C) => c[1] === 'waiting') as C[];
  const s0 = await waitFor(player, 'a customer waits at the bar', (s) => waiting(s).length > 0, 60_000);
  const house0 = s0.game.money.house;

  // Hold E at a good tilt; let go in the green zone.
  // The client predicts the gauge on its own timeline, so releasing when it
  // shows 90% lands in the green on the host. Poll quickly: it rises 2% every 50 ms.
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: -0.4, buttons: INTERACT });
  await player.waitForFunction(() => ((window as any).__lastCall?.game?.pour?.[0] ?? 0) >= 90, null, { polling: 20, timeout: 10_000 });
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: -0.4, buttons: 0 });
  const poured = await waitFor(player, 'a beer in hand', (s) => !!s.game?.beer);
  expect(poured.game.beer[1]).toBe(true);

  // Walk (not run) to the gap between that customer's stool and the next,
  // face the counter, and put the glass down in front of them.
  const target = waiting(await status(player))[0];
  const x = target[2] + 0.5;
  await walkTo(player, [
    [4.0, -1.5],
    [x, -1.5],
    [x, -3.05],
  ]);
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: 0 });
  await player.waitForTimeout(300);
  // Under CI load one short press can reach the host too late and be lost;
  // press Q again while the glass is still in hand.
  for (let i = 0; i < 3 && (await status(player)).holding; i++) {
    await tap(player, DROP);
    await player.waitForTimeout(400);
  }

  const served = await waitFor(
    player,
    'customer served',
    (s) => s.game?.money?.house === house0 + 8 && s.game.pocket === 2 && !s.game.beer,
    10_000,
  );
  expect(served.game.customers.some((c: C) => c[1] === 'drinking')).toBe(true);
  await player.context().close();
  await host.context().close();
});

const USE = 1 << 8;

/** Pour a beer at the tap (the player stands there) and wait until it is in hand. */
async function pourBeer(page: Page) {
  await setInput(page, { mx: 0, my: 0, yaw: 0, pitch: -0.4, buttons: INTERACT });
  await page.waitForFunction(() => ((window as any).__lastCall?.game?.pour?.[0] ?? 0) >= 90, null, { polling: 20, timeout: 10_000 });
  await setInput(page, { mx: 0, my: 0, yaw: 0, pitch: -0.4, buttons: 0 });
  return waitFor(page, 'a beer in hand', (s) => !!s.game?.beer);
}

test('drinking a beer costs $5 and fills the drunk meter', async ({ browser }) => {
  // 300 in each pocket.
  const { host, room } = await createRoom(browser, undefined, '&preset=lastweek');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && s.game?.pocket === 300 && !!s.game?.drunk);
  await walkTo(player, ROUTE_TO_TAP);
  await pourBeer(player);
  await tap(player, USE);
  const drunk = await waitFor(player, 'the beer is drunk', (s) => !s.game?.beer && (s.game?.drunk?.level ?? 0) >= 19);
  expect(drunk.game.pocket).toBe(295);
  expect(drunk.game.drunk.tier).toBe('courage');
  await player.context().close();
  await host.context().close();
});

test('one beer too many: the player passes out, is dragged, and wakes up', async ({ browser }) => {
  test.setTimeout(240_000);
  // Everyone joins Wasted (90): the next beer passes them out.
  const { host, room } = await createRoom(browser, undefined, '&preset=wasted');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && s.game?.drunk?.tier === 'wasted');
  await walkTo(player, ROUTE_TO_TAP);
  // Wasted players stumble every 8 s; a stumble can carry the player off the
  // tap mid-pour, so pour again until the glass is full enough to drink.
  for (let i = 0; i < 4; i++) {
    const s = await pourBeer(player);
    if (s.game.beer[0] >= 60) break;
    await tap(player, DROP);
    await walkTo(player, ROUTE_TO_TAP);
  }
  await tap(player, USE);
  const out = await waitFor(player, 'passed out', (s) => !!s.game?.drunk?.passedOut, 10_000);
  const lying = out.ownPos!;

  // The host walks over, grabs the body with E and backs away with it.
  await walkTo(host, [
    [lying[0], -1.0],
    [lying[0], lying[2] + 1.2],
  ]);
  await setInput(host, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: INTERACT });
  await host.waitForTimeout(300);
  await setInput(host, { mx: 0, my: -1, yaw: 0, pitch: 0, buttons: 0 });
  await host.waitForTimeout(1500);
  await setInput(host, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: DROP });
  const dragged = await waitFor(player, 'body dragged', (s) => !!s.ownPos && s.ownPos[2] > lying[2] + 1.0, 10_000);
  test.info().annotations.push({ type: 'dragged', description: `${lying} -> ${dragged.ownPos}` });

  // 45 seconds after passing out, the player gets up.
  const awake = await waitFor(player, 'awake', (s) => s.game?.drunk && !s.game.drunk.passedOut, 70_000);
  expect(awake.game.drunk.level).toBeLessThan(100);
  await player.context().close();
  await host.context().close();
});

test('a Sloppy speaker sounds lower to everyone else', async ({ browser, browserName }) => {
  test.skip(browserName === 'webkit', 'Playwright WebKit has no fake microphone; see docs/DECISIONS.md');
  // Everyone joins at 45 (Sloppy); the meter drops below 40 after about 12 s.
  const { host, room } = await createRoom(browser, undefined, '&preset=tipsy');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Talker`, 'player');
  const ps = await waitFor(player, 'player joined', (s) => !!s.playerId && !!s.ownPos);
  await player.mouse.click(80, 45);
  await host.bringToFront();
  await host.mouse.click(80, 45);
  await expect
    .poll(() => host.evaluate((id) => (window as any).__lcVoice?.peers().includes(id) ?? false, ps.playerId), {
      timeout: 30_000,
    })
    .toBe(true);

  // Median strongest frequency of the player's voice at the host, sampled
  // only while the fake microphone's beep is sounding.
  const pitchHz = (ms: number) =>
    host.evaluate(
      async ([id, ms]) => {
        const v = (window as any).__lcVoice;
        const seen: number[] = [];
        const end = performance.now() + (ms as number);
        while (performance.now() < end) {
          if (v.level(id) > 0.01) seen.push(v.peakHz(id));
          await new Promise((r) => setTimeout(r, 20));
        }
        seen.sort((a, b) => a - b);
        return seen.length ? seen[Math.floor(seen.length / 2)] : 0;
      },
      [ps.playerId, ms] as const,
    );
  const drunkPitch = await host.evaluate((id) => new Map((window as any).__lastCall.game.voicePitch).get(id), ps.playerId);
  expect(drunkPitch).toBeLessThan(1);
  const low = await pitchHz(3000);

  await waitFor(player, 'sobered up below 40', (s) => (s.game?.drunk?.level ?? 99) < 40, 30_000);
  await host.bringToFront();
  await host.waitForTimeout(500);
  const normal = await pitchHz(3000);
  test.info().annotations.push({ type: 'voice-pitch', description: `drunk ${low.toFixed(0)} Hz, sober ${normal.toFixed(0)} Hz` });
  expect(low).toBeGreaterThan(0);
  expect(low / normal).toBeGreaterThan(0.7);
  expect(low / normal).toBeLessThan(0.9);
  await player.context().close();
  await host.context().close();
});

test('the wasm host replays the recorded shift to the same state as native', async ({ page }) => {
  test.setTimeout(300_000);
  // crates/host/tests/replay.hash holds the native result; the native test
  // checks it too (crates/host/tests/replay.rs).
  const golden = readFileSync('crates/host/tests/replay.hash', 'utf8').trim();
  await page.goto('/?hostonly');
  const started = Date.now();
  const hash = await page.evaluate(async () => {
    // A runtime URL (not a module the test compiler should resolve).
    const url = '/pkg/host.js';
    const host = await import(/* @vite-ignore */ url);
    await host.default();
    return host.host_replay(14 * 60 * 64);
  });
  test.info().annotations.push({ type: 'replay', description: `${hash} in ${((Date.now() - started) / 1000).toFixed(1)} s` });
  expect(hash).toBe(golden);
});
