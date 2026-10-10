// Phase 5: the side games in the browser. Players walk to each station and
// play through `window.__lcGame` (the same requests the keys send).
import { expect, test, type Page } from '@playwright/test';
import { createRoom, openTab, status, waitFor, walkToOnHost, walkTo } from './helpers';

const game = (page: Page, action: any) =>
  page.evaluate((a) => ((window as any).__lcGame ??= []).push(a), action);

const OUT_FRONT: [number, number][] = [
  [0.0, 3.4],
  [0.0, 8.5],
];

/** The perfect basketball aim from (x, z) (crates/shared/src/hoops.rs). */
function perfectAim(x: number, z: number) {
  const [hx, hz, rim, release, g] = [22.0, 2.0, 3.05, 2.0, 9.81];
  const [dx, dz] = [hx - x, hz - z];
  const d = Math.hypot(dx, dz);
  const pitch = 0.96;
  const v = Math.sqrt((g * d * d) / (2 * Math.cos(pitch) ** 2 * (d * Math.tan(pitch) - (rim - release))));
  const power = Math.round(Math.min(1000, Math.max(0, ((v - 3) / 10) * 1000)));
  return { yaw: Math.atan2(-dx, -dz), pitch, power };
}

test('a fisher on the pier hooks a fish and fights it', async ({ browser }) => {
  test.setTimeout(240_000);
  const { host } = await createRoom(browser, undefined, '&chaos=off&preset=casino');
  await walkToOnHost(host, [...OUT_FRONT, [0, 26], [-2, 44.5], [-2, 45.5]]);
  await waitFor(host, 'at the spot', (s) => s.game?.games?.station === 'Fishing(0)');
  await game(host, { Cast: { power: 30 } });
  await waitFor(host, 'line in', (s) => s.game.games.fishing[0].phase === 'waiting', 10_000);
  await waitFor(host, 'a bite', (s) => s.game.games.fishing[0].phase === 'biting', 40_000);
  await game(host, 'Hook');
  await waitFor(host, 'hooked', (s) => s.game.games.fishing[0].phase === 'reeling', 5_000);
  // Reel in the page: hold below the band's middle, let go above it.
  await host.evaluate(() => {
    let held = false;
    (window as any).__reel = setInterval(() => {
      const f = (window as any).__lastCall?.game?.games?.fishing?.[0];
      if (!f || f.phase !== 'reeling') return;
      const want = f.tension < (f.band[0] + f.band[1]) / 2;
      if (want !== held) {
        held = want;
        ((window as any).__lcGame ??= []).push({ Reel: { held } });
      }
    }, 15);
  });
  const s = await waitFor(host, 'the fight ends', (s) => s.game.games.fishing[0].phase === 'idle', 150_000);
  await host.evaluate(() => clearInterval((window as any).__reel));
  const last = s.game.games.fishing[0].last;
  test.info().annotations.push({ type: 'catch', description: JSON.stringify(last) });
  expect(last[0]).toBe(s.playerId);
  expect(['minnow', 'bass', 'catfish', 'shark', 'boot', 'snapped', 'escaped']).toContain(last[1]);
  await host.context().close();
});

test('three of five on the roof: five perfect shots win the pot and draw a crowd', async ({ browser }) => {
  test.setTimeout(180_000);
  const { host } = await createRoom(browser, undefined, '&chaos=off&preset=casino');
  await walkToOnHost(host, [
    [0.0, 3.4],
    [9.0, 3.4],
    [11.5, 3.4],
    [14.0, 2.0],
  ]);
  const start = await waitFor(host, 'on the court', (s) => s.game?.games?.station === 'Court');
  await game(host, { JoinHoops: { mode: 'ThreeOfFive' } });
  await waitFor(host, 'joined', (s) => s.game.games.hoops.entrants.length === 1);
  await game(host, 'StartHoops');
  await waitFor(host, 'started', (s) => s.game.games.hoops.started);
  for (let i = 1; i <= 5; i++) {
    const p = (await status(host)).ownPos!;
    await game(host, { Shoot: perfectAim(p[0], p[2]) });
    await waitFor(host, `shot ${i}`, (s) => s.game.games.hoops.shots === i, 10_000);
  }
  const s = await waitFor(host, 'paid', (s) => s.game.games.hoops.paid.length === 1);
  expect(s.game.games.hoops.paid[0]).toEqual([s.playerId, 20]);
  expect(s.game.games.hoops.crowdSecs).toBeGreaterThan(100);
  expect(s.game.pocket).toBe(start.game.pocket);
  await host.context().close();
});

test('the parking lot: a field goal, a penalty and a gauntlet run', async ({ browser }) => {
  test.setTimeout(180_000);
  const { host } = await createRoom(browser, undefined, '&chaos=off&preset=casino');
  await walkToOnHost(host, [...OUT_FRONT, [-12, 9.5]]);
  await waitFor(host, 'at the tee', (s) => s.game?.games?.station === 'Tee');
  const before = (await status(host)).game.pocket;
  await game(host, { FieldGoal: { yards: 20, stake: 10, kick: { aim: 0, power: 45, curve: 0 } } });
  const fg = await waitFor(host, 'kicked', (s) => s.game.games.fieldGoal.kicks === 1);
  expect(fg.game.games.fieldGoal.last).toEqual([fg.playerId, 20, true, 20]);
  expect(fg.game.pocket).toBe(before + 10);

  await walkToOnHost(host, [[-7, 14.5]]);
  await game(host, { Kick: { kick: { aim: 0.85, power: 60, curve: 0 } } });
  const k = await waitFor(host, 'a penalty', (s) => s.game.games.penalties.kicks === 1, 10_000);
  expect(['goal', 'saved']).toContain(k.game.games.penalties.last[1]);

  await walkToOnHost(host, [
    [0, 20],
    [8, 25],
  ]);
  await waitFor(host, 'in the lane', (s) => s.game.games.station === 'Lane');
  await game(host, { Run: { stake: 10 } });
  await waitFor(host, 'running', (s) => s.game.games.gauntlet.runner === s.playerId, 10_000);
  const r = await waitFor(host, 'tackled standing still', (s) => !!s.game.games.gauntlet.last, 30_000);
  expect(r.game.games.gauntlet.last).toEqual([r.playerId, 'tackled']);
  await host.context().close();
});

test('the fight pit: a hit from across the network, with lag compensation', async ({ browser }) => {
  test.setTimeout(240_000);
  const { host, room } = await createRoom(browser, undefined, '&chaos=off&preset=casino');
  const player = await openTab(browser, `${room.link}&gpu=webgl2&novoice&name=Target`, 'player');
  await waitFor(player, 'joined', (s) => !!s.playerId && s.game?.pocket != null);
  const toPit: [number, number][] = [
    [-8, 3.4],
    [-12, 3.4],
    [-15.5, 3.4],
    [-20, 0],
  ];
  // The host takes the pistol from its rack; the target stands across the pit.
  await Promise.all([walkToOnHost(host, [...toPit, [-25, -4.9]]), walkTo(player, [...toPit, [-25, 3]])]);
  await game(host, { JoinPit: { teams: false } });
  await game(player, { JoinPit: { teams: false } });
  await waitFor(host, 'two fighters', (s) => s.game.games.pit.fighters.length === 2);
  await game(host, { Pick: { weapon: 'Pistol' } });
  await waitFor(host, 'armed', (s) => s.game.games.pit.fighters.some((f: any) => f[0] === s.playerId && f[4] === 'pistol'));
  await game(host, 'StartPit');
  await waitFor(host, 'started', (s) => s.game.games.pit.started);
  await walkToOnHost(host, [[-25, -1]]);
  const target = (await status(player)).playerId;
  for (let i = 0; i < 20; i++) {
    const s = await status(host);
    const me = s.ownPos!;
    const them = (s.players as any[]).find((p) => p[0] === target);
    const [dx, dz] = [them[1] - me[0], them[3] - me[2]];
    const yaw = Math.atan2(-dx, -dz);
    const pitch = Math.atan2(1.3 - 1.6, Math.hypot(dx, dz));
    await game(host, { Fire: { yaw, pitch, view_tick: 0 } });
    await host.waitForTimeout(400);
    if ((await status(host)).game.games.pit.lastHit === target) break;
  }
  const s = await waitFor(host, 'a hit', (s) => s.game.games.pit.lastHit === target, 10_000);
  expect(s.game.games.pit.shots).toBeGreaterThan(0);
  await player.context().close();
  await host.context().close();
});
