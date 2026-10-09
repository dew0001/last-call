//! Determinism (plan section 11): the scripted 14-minute shift gives the same
//! state hash on every run, and the same hash as the committed golden value,
//! which the browser test also checks against the wasm build.

const GOLDEN: &str = include_str!("replay.hash");

#[test]
fn a_recorded_shift_replays_to_the_same_state() {
    let a = host::replay::run(host::replay::SHIFT_TICKS);
    let b = host::replay::run(host::replay::SHIFT_TICKS);
    assert_eq!(a, b, "two runs of the same shift differ");
    assert!(a.rounds > 0 && a.spins > 0 && a.pulls > 0, "customers played every game: {a:?}");
    assert_eq!(
        format!("{:016x}", a.hash),
        GOLDEN.trim(),
        "the replay hash changed. If the simulation changed on purpose, run: cargo run -p last_call_tools -- shift_replay --update"
    );
}
