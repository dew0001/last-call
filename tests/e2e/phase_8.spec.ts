// Phase 8: playtest with real input. Earlier tests drove the game through
// scripted input and missed bugs players hit: taps shorter than a tick,
// fast clicks, empty pockets. These press real keys and the real mouse.
import { test, expect } from '@playwright/test';
import { waitFor, status } from './helpers';
test('real keys: pour and drink with the starting pocket', async ({ page }) => {
  test.setTimeout(300_000);
  await page.goto('/?create&autostart&gpu=webgl2&novoice&name=Host&chaos=off&nodraw');
  await waitFor(page, 'joined', (s) => !!s.playerId && s.game?.pocket != null, 90_000);
  console.log('pocket at start', (await status(page)).game.pocket);
  await page.evaluate(() => (window as any).__lcTeleport(4.0, -5.0));
  await page.waitForTimeout(1500);
  // Pour with E held. Release well short of full: a slow browser reads the
  // status late. A runner that stalls can still overfill the glass (a spill
  // is the game working; tests/e2e/phase_2 checks the pour's timing), so
  // pour again after a spill.
  for (let attempt = 1; ; attempt++) {
    const puddles = (await status(page)).game?.puddles ?? 0;
    await page.keyboard.down('KeyE');
    await waitFor(page, 'pouring', (s) => !!s.game?.pour, 10_000);
    await waitFor(page, 'mostly full', (s) => (s.game?.pour?.[0] ?? 0) >= 70, 20_000);
    await page.keyboard.up('KeyE');
    const s = await waitFor(page, 'poured', (s) => !s.game?.pour, 10_000);
    if (s.game?.beer) break;
    const held = await waitFor(page, 'holding or spilled', (s) => !!s.game?.beer || (s.game?.puddles ?? 0) > puddles, 5_000);
    if (held.game?.beer) break;
    console.log('spilled on attempt', attempt);
    if (attempt === 3) throw new Error('spilled three pours');
    await page.waitForTimeout(500);
  }
  await page.keyboard.press('KeyR');
  const d = await waitFor(page, 'drank it', (s) => !s.game?.beer && (s.game?.drunk?.level ?? 0) > 0, 10_000);
  console.log('after drink', d.game.pocket, d.game.drunk.level);
  expect(d.game.pocket).toBe(95);
});

test('real mouse: shoot a hoop, cast a line, join for fun', async ({ page }) => {
  test.setTimeout(300_000);
  await page.goto('/?create&autostart&gpu=webgl2&novoice&name=Host&chaos=off&nodraw');
  await waitFor(page, 'joined', (s) => !!s.playerId && s.game?.pocket != null, 90_000);
  const box = (await page.locator('#bevy').boundingBox())!;
  const hold = async (ms: number) => {
    await page.mouse.move(box.x + 100, box.y + 100);
    await page.mouse.down();
    await page.waitForTimeout(ms);
    await page.mouse.up();
  };
  // The roof court.
  await page.evaluate(() => (window as any).__lcTeleport(19.0, 2.0));
  await waitFor(page, 'on the court', (s) => s.game?.games?.station === 'Court', 30_000);
  await page.locator('#bevy').focus();
  await page.keyboard.press('Digit0');
  await page.keyboard.press('Digit1');
  const j = await waitFor(page, 'joined for fun', (s) => (s.game?.games?.hoops?.entrants?.length ?? 0) === 1, 10_000);
  console.log('fun join: pot', j.game.games.hoops.pot, 'pocket', j.game.pocket);
  expect(j.game.games.hoops.pot).toBe(0);
  expect(j.game.pocket).toBe(100);
  await hold(800);
  const shot = await waitFor(page, 'a shot', (s) => (s.game?.games?.hoops?.shots ?? 0) >= 1, 10_000);
  console.log('shots', shot.game.games.hoops.shots, JSON.stringify(shot.game.games.hoops.last));
  // The pier: cast.
  await page.evaluate(() => (window as any).__lcTeleport(-2.0, 44.6));
  await waitFor(page, 'at a fishing spot', (s) => String(s.game?.games?.station ?? '').startsWith('Fishing'), 30_000);
  await hold(700);
  const f = await waitFor(page, 'line out', (s) => s.game?.games?.fishing?.[0]?.phase !== 'idle', 10_000);
  console.log('fishing phase', f.game.games.fishing[0].phase);
});
