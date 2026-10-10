// Achievements and cosmetics (plan sections 4.5 and 10, Phase 7). Personal
// unlocks persist across runs in localStorage, tied to this browser's
// player UUID. They are cosmetic only: each achievement unlocks a hat,
// worn in every room you join (the host gets the choice in Join).
//
// The page watches the game status (`window.__lastCall`) twice a second.

const KEY = 'lastcall-unlocks';

/** id, name, how to earn it, the hat style it unlocks (1 to 3). */
export const ACHIEVEMENTS = [
  { id: 'twentyone', name: 'Blackjack x3', how: 'Hit 21 three times in a shift', hat: 1 },
  { id: 'roof', name: 'Sky high', how: 'Pass out on the roof', hat: 2 },
  { id: 'boot', name: 'Catch the boot', how: 'Reel in the old boot', hat: 3 },
  { id: 'shark', name: 'Jaws', how: 'Land a shark', hat: 3 },
  { id: 'gauntlet', name: 'Untouchable', how: 'Score in the gauntlet', hat: 1 },
  { id: 'pit', name: 'Last one standing', how: 'Win a fight pit round', hat: 2 },
];

function load(uuid) {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    return s.uuid === uuid ? { earned: s.earned ?? [], hat: s.hat ?? 0 } : { earned: [], hat: 0 };
  } catch {
    return { earned: [], hat: 0 };
  }
}

function save(uuid, state) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ uuid, ...state }));
  } catch {
    // Private mode: unlocks last for this tab.
  }
}

/** Hats this player may wear: 0 (none) plus every unlocked style. */
export function unlockedHats(uuid) {
  const { earned } = load(uuid);
  return [0, ...new Set(ACHIEVEMENTS.filter((a) => earned.includes(a.id)).map((a) => a.hat))].sort();
}

export function chosenHat(uuid) {
  const s = load(uuid);
  return unlockedHats(uuid).includes(s.hat) ? s.hat : 0;
}

export function chooseHat(uuid, hat) {
  const s = load(uuid);
  if (unlockedHats(uuid).includes(hat)) save(uuid, { ...s, hat });
}

function toast(text) {
  const el = Object.assign(document.createElement('div'), { className: 'toast', textContent: text });
  document.body.append(el);
  setTimeout(() => el.remove(), 5000);
}

/** The roof's floor (crates/shared/src/world.rs). */
const onRoof = (p) => p && p[0] >= 10 && p[0] <= 24 && p[2] >= -5 && p[2] <= 9;

/** Start watching the game for achievements. */
export function watchAchievements(uuid) {
  const state = load(uuid);
  window.__lcAchievements = state.earned;
  const earn = (id) => {
    if (state.earned.includes(id)) return;
    state.earned.push(id);
    save(uuid, state);
    const a = ACHIEVEMENTS.find((x) => x.id === id);
    toast(`Achievement: ${a.name}. ${a.how}. New hat unlocked!`);
  };
  // Blackjack 21s this shift: rounds counted once each.
  let shiftKey = '';
  const twentyOnes = new Set();
  setInterval(() => {
    const s = window.__lastCall;
    const g = s?.game;
    if (!g || !s.playerId) return;
    const me = s.playerId;
    const key = g.shift ? `${g.shift.week}.${g.shift.shift}` : '';
    if (key !== shiftKey) {
      shiftKey = key;
      twentyOnes.clear();
    }
    const bj = g.casino?.blackjack;
    const seat = bj?.mySeat;
    if (bj && seat != null && bj.seats[seat]?.hands?.some((h) => h[1] === 21)) {
      twentyOnes.add(bj.rounds);
      if (twentyOnes.size >= 3) earn('twentyone');
    }
    if (g.drunk?.passedOut && onRoof(s.ownPos)) earn('roof');
    for (const f of g.games?.fishing ?? []) {
      if (f.last?.[0] === me && f.last[1] === 'boot') earn('boot');
      if (f.last?.[0] === me && f.last[1] === 'shark') earn('shark');
    }
    if (g.games?.gauntlet?.last?.[0] === me && g.games.gauntlet.last[1] === 'scored') earn('gauntlet');
    if (g.games?.pit?.paid?.some((p) => p[0] === me)) earn('pit');
  }, 500);
}
