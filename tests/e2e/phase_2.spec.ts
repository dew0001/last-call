// Phase 2: shift loop and economy.
// Rooms run with `&fast=60`: every shift phase is 60 times shorter, so a
// 14-minute shift takes 14 seconds (Setup 2, Open 9, Last call 2, Payment 1).
import { expect, test } from '@playwright/test';
import { createRoom, openTab, status, waitFor } from './helpers';

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
