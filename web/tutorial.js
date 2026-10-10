// The tutorial shift (plan section 10, Phase 7): `?create&tutorial` opens a
// room with no chaos and every customer at the bar, and walks the player
// through the first shift with prompts. Each prompt waits for the game
// status (`window.__lastCall`) to show it was done.

const STEPS = [
  {
    text: 'Welcome to LAST CALL. Walk with WASD and look with the mouse.',
    done: (s, start) => !!s.ownPos && !!start.ownPos && Math.hypot(s.ownPos[0] - start.ownPos[0], s.ownPos[2] - start.ownPos[2]) > 1.5,
  },
  {
    text: 'Pour a beer: stand at the tap behind the east end of the counter and hold E. Let go in the green.',
    done: (s) => !!s.game?.beer,
  },
  {
    text: 'Serve it: carry the glass to a customer waiting at the counter and drop it in front of them (Q). Walk, do not run.',
    done: (s, start) => (s.game?.money?.house ?? 0) > (start.game?.money?.house ?? 0),
  },
  {
    text: 'Bank it: in the office (east door), press E at the safe to move $100 from your pocket into the house pool.',
    done: (s, start) => (s.game?.pocket ?? 0) < (start.game?.pocket ?? 0) && (s.game?.money?.house ?? 0) > (start.game?.money?.house ?? 0),
  },
  {
    text: 'Try a table: blackjack, roulette or the slots. Stand at one to see its keys.',
    done: (s) => !!s.game?.casino?.near,
  },
  {
    text: "At Payment the loan shark takes the week's due from the house pool. Pay off $120,000 to buy the bar. Good luck!",
    done: () => false,
  },
];

export function startTutorial() {
  const box = Object.assign(document.createElement('div'), { id: 'tutorial' });
  document.body.append(box);
  let step = 0;
  let start = null;
  const showStep = () => {
    box.textContent = `${STEPS[step].text}  (${step + 1}/${STEPS.length})`;
  };
  const timer = setInterval(() => {
    const s = window.__lastCall;
    if (!s?.playerId || !s.game) return;
    if (!start) {
      start = structuredClone(s);
      showStep();
    }
    if (STEPS[step].done(s, start)) {
      step += 1;
      start = structuredClone(s);
      showStep();
      if (step === STEPS.length - 1) {
        clearInterval(timer);
        setTimeout(() => box.remove(), 20_000);
      }
    }
  }, 300);
  showStep();
}
