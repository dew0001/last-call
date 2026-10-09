// Phase 2: shift loop and economy.
// Rooms run with `&fast=60`: every shift phase is 60 times shorter, so a
// 14-minute shift takes 14 seconds (Setup 2, Open 9, Last call 2, Payment 1).
import { expect, test } from '@playwright/test';
import { INTERACT, ROUTE_TO_SAFE, createRoom, openTab, status, tap, waitFor, walkTo } from './helpers';

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

test('money goes into the safe, the last payment wins, and a new run starts', async ({ browser }) => {
  // Week 6, 80,000 paid, 45,000 in the house, 300 in each pocket.
  const { host, room } = await createRoom(browser, undefined, '&fast=60&preset=lastweek');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Rook`, 'player');
  const s0 = await waitFor(player, 'player joined', (s) => !!s.playerId && s.game?.pocket === 300 && !!s.game?.money);
  expect(s0.game.money.due).toBe(40_000);

  // Walk into the office and press E at the safe three times.
  await walkTo(player, ROUTE_TO_SAFE);
  for (let i = 0; i < 3; i++) await tap(player, INTERACT);
  const s1 = await waitFor(player, 'pocket emptied into the house', (s) => s.game?.pocket === 0);
  expect(s1.game.money.house).toBe(45_300);

  // At the end of week 6 the loan shark takes the 40,000 left: the run is won.
  const won = await waitFor(player, 'run won', (s) => s.game?.money?.outcome === 'won', 90_000);
  expect(won.game.money.paid).toBe(120_000);
  expect(won.game.money.last).toEqual(['paid', 40_000]);
  expect((await status(host)).game.money.outcome).toBe('won');

  // Then a new run at new game plus 1, with no money carried over.
  const next = await waitFor(player, 'new run', (s) => s.game?.money?.outcome === 'playing' && s.game.money.ng === 1);
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
