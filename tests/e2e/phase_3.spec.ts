// Phase 3: the casino. Real browser tabs play blackjack, roulette and the
// slots against the host Worker; the RNG audit log exported from IndexedDB
// re-derives every outcome in the browser.
import { expect, test, type Page } from '@playwright/test';
import { INTERACT, createRoom, openTab, setInput, status, tap, waitFor, walkTo, walkToOnHost } from './helpers';

type Table = 'Blackjack' | 'Roulette' | { Slot: number };

/** Send a table request, as the table keys do (`window.__lcTable`). */
const ask = (page: Page, table: Table, action: unknown) =>
  page.evaluate((r) => ((window as any).__lcTable ??= []).push(r), { table, action });

const casino = async (page: Page) => (await status(page)).game?.casino;
const me = async (page: Page) => `p:${(await status(page)).playerId}`;
const pocket = async (page: Page) => (await status(page)).game?.pocket as number;

// Routes from the spawn points along the front of the room (z = 4).
const TO_DEALER: [number, number][] = [
  [-2.5, 3.0],
  [-2.5, -0.55],
  [-5.0, -0.55],
];
const TO_CROUPIER: [number, number][] = [
  [2.0, 3.0],
  [2.0, -0.6],
  [4.5, -0.6],
];
const toSeat = (x: number, z: number): [number, number][] => [
  [x, 3.0],
  [x, z],
];
/** Blackjack seat 3 and roulette spot 3. */
const BJ_SEAT: [number, number] = [-5.0, 2.0];
const RL_SPOT: [number, number] = [4.7, 2.05];
const SLOT0: [number, number] = [-8.8, -1.5];

const Cap = (a: string) => a[0].toUpperCase() + a.slice(1);

/** Play one blackjack round: the player stands, the dealer follows the rules. */
async function playRound(dealer: Page, player: Page | null) {
  const before = (await casino(dealer)).blackjack.rounds;
  await ask(dealer, 'Blackjack', 'Deal');
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const b = (await casino(dealer)).blackjack;
    if (b.rounds > before) return;
    if (player) {
      const pb = (await casino(player)).blackjack;
      const seat = pb.mySeat;
      if (pb.phase === 'insurance' && seat != null && pb.seats[seat].insurance == null) await ask(player, 'Blackjack', { Insure: false });
      if (pb.phase === 'players' && pb.toAct === seat) await ask(player, 'Blackjack', { Play: 'Stand' });
    }
    if (b.dealerShould) await ask(dealer, 'Blackjack', { Dealer: Cap(b.dealerShould) });
    await dealer.waitForTimeout(250);
  }
  throw new Error('the round did not finish');
}

test('blackjack: a dealer deals, the money adds up, and a win waits on the felt', async ({ browser }) => {
  test.setTimeout(300_000);
  // Setup lasts 2 minutes at normal speed: no customers, just the two players.
  const { host, room } = await createRoom(browser, undefined, '&preset=casino');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Punter`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && (s.playersSeen ?? 0) >= 2);
  await Promise.all([walkToOnHost(host, TO_DEALER), walkTo(player, toSeat(...BJ_SEAT))]);
  expect((await casino(host)).near).toBe('blackjack');

  await ask(host, 'Blackjack', 'TakeRole');
  const dealer = await me(host);
  await waitFor(host, 'the host deals', (s) => s.game?.casino?.blackjack?.dealer === dealer);
  await ask(player, 'Blackjack', { Bet: 15 });
  await player.waitForTimeout(1000);
  expect((await casino(player)).blackjack.mySeat, 'an odd bet is refused').toBeNull();
  await ask(player, 'Blackjack', { Bet: 20 });
  await waitFor(player, 'seated with a bet', (s) => s.game?.casino?.blackjack?.mySeat != null);

  const money = async () =>
    (await status(host)).game.money.house + (await pocket(host)) + (await pocket(player)) + (await casino(host)).chips;
  const start = await money();
  expect(start).toBe(2_000);

  let won = false;
  for (let round = 0; round < 25 && !won; round++) {
    await playRound(host, player);
    await host.waitForTimeout(500);
    expect(await money(), 'money is neither made nor lost').toBe(start);
    const b = (await casino(player)).blackjack;
    expect(b.seats[b.mySeat].last).not.toBeNull();
    won = (await casino(player)).chips > 0;
  }
  expect(won, 'a win in 25 rounds').toBe(true);
  expect(await pocket(host), 'the dealer earns commission only').toBeGreaterThanOrEqual(1_000);

  // The chip stack on the felt: face the table and pick it up.
  const chips = (await casino(player)).chips;
  const before = await pocket(player);
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: -0.5, buttons: 0 });
  await player.waitForTimeout(300);
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: -0.5, buttons: INTERACT });
  await player.waitForTimeout(300);
  await setInput(player, { mx: 0, my: 0, yaw: 0, pitch: -0.5, buttons: 0 });
  await waitFor(player, 'the chips are in the pocket', (s) => s.game?.pocket === before + chips, 10_000);
  test.info().annotations.push({ type: 'blackjack', description: `picked up $${chips}; dealer pocket ${await pocket(host)}` });
  await player.context().close();
  await host.context().close();
});

test('roulette: the result is known when the spin starts, winners are paid, the croupier rakes', async ({ browser }) => {
  test.setTimeout(240_000);
  const { host, room } = await createRoom(browser, undefined, '&preset=casino');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Punter`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && (s.playersSeen ?? 0) >= 2);
  await Promise.all([walkToOnHost(host, TO_CROUPIER), walkTo(player, toSeat(...RL_SPOT))]);
  await ask(host, 'Roulette', 'TakeRole');
  const croupier = await me(host);
  await waitFor(host, 'the host runs the wheel', (s) => s.game?.casino?.roulette?.croupier === croupier);

  await ask(player, 'Roulette', { RouletteBet: ['Red', 10] });
  await ask(player, 'Roulette', { RouletteBet: ['Black', 10] });
  await ask(player, 'Roulette', { RouletteBet: [{ Straight: 17 }, 5] });
  await ask(player, 'Roulette', { RouletteBet: [{ Split: [3, 4] }, 5] });
  await waitFor(player, 'three bets down', (s) => s.game?.casino?.roulette?.bets?.length === 3);
  await player.waitForTimeout(500);
  expect(await pocket(player), 'the bad split is refused').toBe(975);

  await ask(player, 'Roulette', 'Spin');
  await player.waitForTimeout(1000);
  expect((await casino(player)).roulette.spinning, 'only the croupier spins').toBe(false);
  await ask(host, 'Roulette', 'Spin');
  const spinning = await waitFor(player, 'spinning', (s) => s.game?.casino?.roulette?.spinning === true, 10_000);
  const result = spinning.game.casino.roulette.result as number;
  expect(result).toBeGreaterThanOrEqual(0);
  expect(result).toBeLessThanOrEqual(36);
  const t0 = Date.now();
  const done = await waitFor(player, 'the ball drops', (s) => s.game?.casino?.roulette?.spins === 1, 20_000);
  expect(Date.now() - t0, 'at least a 6 s spin').toBeGreaterThan(4_500);
  expect(done.game.casino.roulette.last).toBe(result);

  const reds = [1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36];
  const red = reds.includes(result);
  const black = result !== 0 && !red;
  const expected = (red ? 20 : 0) + (black ? 20 : 0) + (result === 17 ? 180 : 0);
  await player.waitForTimeout(1000);
  expect((await casino(player)).chips, `paid on the rail for ${result}`).toBe(expected);
  const losers = [!red, !black, result !== 17].filter(Boolean).length;
  await waitFor(host, 'losing chips on the layout', (s) => s.game?.casino?.roulette?.toRake === losers, 5_000);

  // No spin until the layout is clear.
  await ask(player, 'Roulette', { RouletteBet: ['Odd', 10] });
  await ask(host, 'Roulette', 'Spin');
  await host.waitForTimeout(1500);
  expect((await casino(host)).roulette.spinning).toBe(false);
  for (let i = 0; i < 20 && (await casino(host)).roulette.toRake > 0; i++) {
    await ask(host, 'Roulette', 'Rake');
    await host.waitForTimeout(400);
  }
  expect((await casino(host)).roulette.toRake, 'raked off').toBe(0);
  await ask(host, 'Roulette', 'Spin');
  await waitFor(host, 'the next spin', (s) => s.game?.casino?.roulette?.spinning === true, 10_000);
  await player.context().close();
  await host.context().close();
});

test('slots: the reels land on the host\'s stops and pay by the paytable', async ({ browser }) => {
  test.setTimeout(180_000);
  const { host } = await createRoom(browser, undefined, '&preset=casino');
  await walkToOnHost(host, toSeat(...SLOT0));
  expect((await casino(host)).near).toBe('slot0');
  const pays = (l: string[]) => {
    const [a, b, c] = l;
    if (a === b && b === c) return { seven: 150, bar: 50, bell: 10, lemon: 8, cherry: 5 }[a] ?? 0;
    if (a === 'cherry' && b === 'cherry') return 2;
    return a === 'cherry' ? 1 : 0;
  };
  let money = await pocket(host);
  for (let i = 1; i <= 5; i++) {
    await ask(host, { Slot: 0 }, { Pull: 5 });
    const s = await waitFor(host, `pull ${i}`, (s) => {
      const m = s.game?.casino?.slots?.[0];
      return m?.pulls === i && !m.spinning;
    }, 10_000);
    const m = s.game.casino.slots[0];
    expect(m.lastReturn, m.line.join(' ')).toBe(5 * pays(m.line));
    await host.waitForTimeout(300);
    const now = await pocket(host);
    expect(now).toBe(money - 5 + m.lastReturn);
    money = now;
  }
  await host.context().close();
});

test('a Wasted player cannot deal; Courage bets 1.5 times the table maximum', async ({ browser }) => {
  test.setTimeout(180_000);
  const wasted = await createRoom(browser, undefined, '&preset=wasted');
  await walkTo(wasted.host, TO_DEALER);
  await ask(wasted.host, 'Blackjack', 'TakeRole');
  await wasted.host.waitForTimeout(1500);
  expect((await casino(wasted.host)).blackjack.dealer).toBeNull();
  await wasted.host.context().close();

  const tipsy = await createRoom(browser, undefined, '&preset=tipsy');
  await walkToOnHost(tipsy.host, toSeat(...BJ_SEAT));
  await ask(tipsy.host, 'Blackjack', { Bet: 150 });
  const s = await waitFor(tipsy.host, 'a $150 bet', (s) => {
    const b = s.game?.casino?.blackjack;
    return b?.mySeat != null && b.seats[b.mySeat].bet === 150;
  }, 10_000);
  expect(s.game.drunk.tier).not.toBe('sober');
  await tipsy.host.context().close();
});

test('customers gamble at every game, and the audit log replays in the browser', async ({ browser }) => {
  test.setTimeout(300_000);
  // fast=20: Setup 6 s, waves every 4.5 s.
  const { host } = await createRoom(browser, undefined, '&chaos=off&fast=20&preset=casino');
  await walkToOnHost(host, TO_DEALER);
  await ask(host, 'Blackjack', 'TakeRole');
  const where = new Set<string>();
  const deadline = Date.now() + 120_000;
  let c: any;
  for (;;) {
    c = await casino(host);
    const s = await status(host);
    for (const [, mood, x, z] of s.game?.customers ?? []) if (mood === 'gambling') where.add(x < -8 ? 'slots' : x < 0 ? 'blackjack' : 'roulette');
    const pulls = c.slots.reduce((n: number, m: any) => n + m.pulls, 0);
    if (where.size === 3 && c.blackjack.rounds >= 2 && pulls >= 5) break;
    if (Date.now() > deadline) throw new Error(`customers: ${[...where]}; ${JSON.stringify(c).slice(0, 400)}`);
    // Deal for the customers.
    const b = c.blackjack;
    if (b.phase === 'betting' && b.seats.some((s: any) => s.bet > 0)) await ask(host, 'Blackjack', 'Deal');
    if (b.dealerShould) await ask(host, 'Blackjack', { Dealer: Cap(b.dealerShould) });
    await host.waitForTimeout(300);
  }
  test.info().annotations.push({ type: 'customers', description: `${[...where]}; ${c.blackjack.rounds} rounds` });
  // Host tick cost with every table busy (budget: 6 ms).
  const ticks = await host.evaluate(() => (window as any).__hostTicks.slice(-20));
  const avg = ticks.reduce((n: number, t: any) => n + t.tickAvgMs, 0) / ticks.length;
  const max = Math.max(...ticks.map((t: any) => t.tickMaxMs));
  test.info().annotations.push({ type: 'host-tick', description: `avg ${avg.toFixed(2)} ms, worst ${max.toFixed(2)} ms` });
  expect(avg).toBeLessThan(6);

  // Give the Worker a second to hand over its last chunk, then export.
  await host.waitForTimeout(1500);
  const report = await host.evaluate(async () => {
    const text: string = await (window as any).__lastCallAudit.export();
    const url = '/pkg/host.js';
    const wasm = await import(/* @vite-ignore */ url);
    await wasm.default();
    return { lines: text.split('\n').length, report: JSON.parse(wasm.host_audit_verify(text)) };
  });
  test.info().annotations.push({ type: 'audit', description: JSON.stringify(report) });
  expect(report.report.error).toBeUndefined();
  expect(report.report.shuffles).toBeGreaterThanOrEqual(1);
  expect(report.report.reels).toBeGreaterThanOrEqual(5);
  expect(report.report.draws).toBeGreaterThan(100);
  await host.context().close();
});

test('table keys: T takes the deal, digits bet', async ({ browser }) => {
  test.setTimeout(180_000);
  const { host, room } = await createRoom(browser, undefined, '&preset=casino');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&name=Punter`, 'player');
  await waitFor(player, 'player joined', (s) => !!s.playerId && (s.playersSeen ?? 0) >= 2);
  await Promise.all([walkToOnHost(host, TO_DEALER), walkTo(player, toSeat(...BJ_SEAT))]);
  // Scripted input stays on (standing still); keys still reach the table.
  await host.locator('#bevy').focus();
  await host.keyboard.press('KeyT');
  const dealer = await me(host);
  await waitFor(host, 'T took the deal', (s) => s.game?.casino?.blackjack?.dealer === dealer, 10_000);
  await player.locator('#bevy').focus();
  await player.keyboard.press('Digit2');
  await waitFor(player, '2 bet $20', (s) => {
    const b = s.game?.casino?.blackjack;
    return b?.mySeat != null && b.seats[b.mySeat].bet === 20;
  }, 10_000);
  await host.keyboard.press('Enter');
  // A natural blackjack settles at once, so a finished round counts too.
  await waitFor(player, 'Enter dealt', (s) => {
    const b = s.game?.casino?.blackjack;
    return (b?.rounds ?? 0) > 0 || (b?.seats ?? []).some((x: any) => x.hands.length > 0);
  }, 10_000);
  await tap(player, 0);
  await player.context().close();
  await host.context().close();
});

test('a saved run resumes in a new room with the same money and shift', async ({ browser }) => {
  test.setTimeout(240_000);
  // fast=60: a 14-second shift. The host plays the slots so its pocket and
  // the house pool move, then the next Setup saves the run.
  const { host } = await createRoom(browser, undefined, '&chaos=off&fast=60&preset=casino');
  const id = (await status(host)).playerId as string;
  await walkToOnHost(host, toSeat(...SLOT0));
  for (let i = 1; i <= 3; i++) {
    await ask(host, { Slot: 0 }, { Pull: 5 });
    await waitFor(host, `pull ${i}`, (s) => s.game?.casino?.slots?.[0]?.pulls === i && !s.game.casino.slots[0].spinning, 15_000);
  }
  const latest = () =>
    host.evaluate(async () => {
      const saves = await import(/* @vite-ignore */ '/saves.js');
      const s = await saves.latest();
      return s ? JSON.parse(s.json) : null;
    });
  let save: any = null;
  for (let i = 0; i < 120 && !(save?.calendar?.shift >= 1); i++) {
    await host.waitForTimeout(500);
    save = await latest();
  }
  expect(save?.calendar?.shift, 'saved at the next Setup').toBeGreaterThanOrEqual(1);
  expect(save.pockets[id]).toBeDefined();
  test.info().annotations.push({ type: 'save', description: JSON.stringify(save) });

  // Close the tab; the same browser profile (same player id) resumes the run.
  const context = host.context();
  await host.close();
  const title = await context.newPage();
  await title.goto('/?gpu=webgl2');
  await expect(title.locator('#resume')).toBeVisible({ timeout: 60_000 });
  await expect(title.locator('#resume')).toContainText(`week ${save.calendar.week}, shift ${save.calendar.shift + 1}`);
  await title.close();

  const resumed = await context.newPage();
  await resumed.goto('/?create&resume&gpu=webgl2&nodraw&novoice&name=Host');
  const s = await waitFor(resumed, 'resumed', (s) => !!s.playerId && !!s.game?.money && s.game.pocket != null, 60_000);
  expect(s.playerId).toBe(id);
  expect(s.game.shift.week).toBe(save.calendar.week);
  expect(s.game.shift.shift).toBe(save.calendar.shift + 1);
  expect(s.game.shift.phase).toBe('SETUP');
  expect(s.game.money.house).toBe(save.ledger.house);
  expect(s.game.pocket).toBe(save.pockets[id]);
  await context.close();
});
