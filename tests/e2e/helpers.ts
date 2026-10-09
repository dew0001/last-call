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
