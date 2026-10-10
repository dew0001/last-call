// Phase 4: fixtures (Zeen and The Spins, the upgrade shop) and a chaos event
// countered in the browser (the outage and the basement breaker).
import { expect, test } from '@playwright/test';
import { createRoom, status, waitFor, walkToOnHost } from './helpers';

/** From the main room through the office door. */
const TO_OFFICE: [number, number][] = [
  [8.2, 3.0],
  [8.2, -1.0],
  [8.2, -3.0],
];

const fixture = (page: any, fixture: any, action: any) =>
  page.evaluate(([f, a]: any) => ((window as any).__lcFixture ??= []).push({ fixture: f, action: a }), [fixture, action]);

test('two pouches of Zeen on top of beer bring on The Spins', async ({ browser }) => {
  test.setTimeout(180_000);
  // Tipsy: 45 on the drunk meter. Two pouches put Focus at 70: The Spins.
  const { host } = await createRoom(browser, undefined, '&chaos=off&preset=tipsy');
  await walkToOnHost(host, [...TO_OFFICE, [7.0, -5.6]]);
  const near = await waitFor(host, 'at the drawer', (s) => s.game?.phase4?.nearFixture === 'Office drawer');
  expect(near.game.phase4.menu[0]).toBe('Zeen $3');
  const pocket = near.game.pocket;
  await fixture(host, 'ZeenDrawer', { Buy: 'Zeen' });
  await waitFor(host, 'one pouch', (s) => s.game.phase4.focus >= 30 && s.game.pocket === pocket - 3);
  await fixture(host, 'ZeenDrawer', { Buy: 'Zeen' });
  await waitFor(host, 'the spins', (s) => s.game.phase4.spinning, 10_000);
  const after = await waitFor(host, 'vomit', (s) => !s.game.phase4.spinning && s.game.phase4.vomit === 1, 15_000);
  expect(after.game.phase4.focus).toBe(0);
  expect(after.game.drunk.level).toBe(0);
  await host.context().close();
});

test('the upgrade terminal sells a Felt Upgrade from the house pool', async ({ browser }) => {
  test.setTimeout(180_000);
  // The last week: 45,000 in the house. Plan timings: a two-minute Setup.
  const { host } = await createRoom(browser, undefined, '&chaos=off&preset=lastweek');
  await walkToOnHost(host, [...TO_OFFICE, [7.0, -3.4]]);
  const s = await waitFor(host, 'at the terminal', (s) => s.game?.phase4?.nearFixture === 'Upgrade terminal');
  expect(s.game.shift.phase).toBe('SETUP');
  const house = s.game.money.house;
  await fixture(host, 'Shop', { Upgrade: 'Felt' });
  const after = await waitFor(host, 'bought', (s) => s.game.phase4.upgrades[0] === 1);
  expect(after.game.money.house).toBe(house - 1500);
  expect(after.game.phase4.menu[0]).toContain('rank 2/3');
  await host.context().close();
});

test('the breaker ends a power outage', async ({ browser }) => {
  test.setTimeout(240_000);
  // fast=10: Setup 12 s, Open 54 s.
  const { host } = await createRoom(browser, undefined, '&chaos=off&fast=10');
  await waitFor(host, 'open', (s) => s.game?.shift?.phase === 'OPEN', 60_000);
  await host.evaluate(() => (window as any).__forceChaos('Outage'));
  const dark = await waitFor(host, 'the lights go out', (s) => s.game.phase4.dark, 10_000);
  expect(dark.game.phase4.chaos[0][0]).toBe('Outage');
  // Through the bar's west door into the stairwell.
  await walkToOnHost(host, [
    [-8.0, 3.4],
    [-11.0, 3.4],
    [-12.0, 4.0],
  ]);
  await waitFor(host, 'at the breaker', (s) => s.game.phase4.nearFixture === 'Breaker');
  await fixture(host, 'Breaker', 'Use');
  const lit = await waitFor(host, 'the lights come back', (s) => !s.game.phase4.dark, 10_000);
  expect(lit.game.phase4.recent).toContainEqual(['Outage', 'countered']);
  expect((await status(host)).game.phase4.chaos).toHaveLength(0);
  await host.context().close();
});
