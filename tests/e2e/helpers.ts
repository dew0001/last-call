// Shared helpers for the browser tests: page status, waiting, tabs, rooms.
import { expect, type Browser, type Page } from '@playwright/test';

export type Status = {
  frames: number;
  propsSeen?: number;
  holding?: boolean;
  propsOnFloor?: number;
  connected?: boolean;
  playerId?: string | null;
  playersSeen?: number;
  ownPos?: [number, number, number] | null;
  /** Game state (`window.__lastCall.game`), see crates/client/src/online.rs. */
  game?: any;
};

export const status = (page: Page) =>
  page.evaluate(() => ({ ...((window as any).__lastCall ?? {}), error: (window as any).__lastCallError })) as Promise<
    Status & { error?: string }
  >;

/** Poll a page until `check` passes, failing fast on a client error. */
export async function waitFor(page: Page, what: string, check: (s: Status) => boolean, timeoutMs = 60_000) {
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
export async function openTab(browser: Browser, url: string, tag: string, size = { width: 160, height: 90 }) {
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

/** Host a room. `extra` adds URL parameters, for example `&fast=60`. */
export async function createRoom(browser: Browser, size?: { width: number; height: number }, extra = '') {
  const host = await openTab(browser, `/?create&gpu=webgl2&name=Host${extra}`, 'host', size);
  await expect.poll(() => host.evaluate(() => (window as any).__lcRoom?.code ?? null), { timeout: 60_000 }).toMatch(
    /^[BCDFGHJKLMNPQRSTVWXYZ]{5}$/,
  );
  const room = await host.evaluate(() => (window as any).__lcRoom);
  await waitFor(host, 'host joins its own room', (s) => !!s.playerId && (s.playersSeen ?? 0) >= 1);
  return { host, room: room as { code: string; link: string } };
}

export type Input = { mx: number; my: number; yaw: number; pitch: number; buttons: number };
export const setInput = (page: Page, input: Input) => page.evaluate((i) => ((window as any).__lcInput = i), input);
export const INTERACT = 1 << 3;
export const DROP = 1 << 5;

/**
 * Walk the player through (x, z) waypoints with scripted input, steering from
 * its predicted position. Slows down near each point so it does not overshoot.
 */
export async function walkTo(page: Page, points: [number, number][], timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  for (const [x, z] of points) {
    for (;;) {
      if (Date.now() > deadline) throw new Error(`walkTo: did not reach ${x},${z}; at ${(await status(page)).ownPos}`);
      const p = (await status(page)).ownPos;
      if (!p) {
        await page.waitForTimeout(100);
        continue;
      }
      const [dx, dz] = [x - p[0], z - p[2]];
      const d = Math.hypot(dx, dz);
      if (d < 0.25) break;
      // Forward is (-sin yaw, -cos yaw).
      await setInput(page, { mx: 0, my: Math.min(1, Math.max(0.25, d / 1.5)), yaw: Math.atan2(-dx, -dz), pitch: 0, buttons: 0 });
      await page.waitForTimeout(50);
    }
  }
  await setInput(page, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: 0 });
}

/** From the main room, past the roulette table, through the office door, to the office safe. */
export const ROUTE_TO_SAFE: [number, number][] = [
  [8.2, 3.0],
  [8.2, -1.0],
  [8.2, -3.0],
  [9.4, -5.55],
];

/** From the main room to the front of the beer tap, between two stools. */
export const ROUTE_TO_TAP: [number, number][] = [
  [4.0, -1.5],
  [4.0, -3.0],
];

/** Tap a button: down for `ms`, then up for `ms`. */
export async function tap(page: Page, buttons: number, ms = 150) {
  await setInput(page, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons });
  await page.waitForTimeout(ms);
  await setInput(page, { mx: 0, my: 0, yaw: 0, pitch: 0, buttons: 0 });
  await page.waitForTimeout(ms);
}
