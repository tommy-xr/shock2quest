import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";

// End-to-end test for the in-world lock traps (GitHub #590).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: `TrapUnlock` was mapped to a no-op script, and
// `is_entity_locked` refuses a `PropLocked(true)` entity that has no
// `PropKeyDst` unconditionally ("nothing can unlock it"). So a button the
// original game hands out via an unlock trigger stayed locked for the whole
// game - firing the trap changed nothing and the button kept answering
// `hackfail`.
//
// eng1 wiring (stable mission object ids, reported as `template_id`):
//   QB Filter 155 -> Once Router 878 -> Unlock Trap 1213 -> buttons 990, 1787, 1210
//   grav-lift button 1787 -> Lift 1 (1856)
//   storage-4 button 948 -> its own Unlock Trap 838 (deliberately NOT fired)
// Runtime entity ids are not stable across runs, so everything is discovered by
// mission object id.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const ONCE_ROUTER = 878; // power restoration fires this, which fires the trap
const UNLOCK_TRAP = 1213; // the trap itself, for the off-edge (re-lock) check
const GRAV_LIFT_BUTTON = 1787;
const GRAV_LIFT = 1856;
const UNLOCKED_TWIN_BUTTON = 1871; // control: never locked
const UNRELATED_LOCKED_BUTTON = 948; // control: its unlock trap stays unfired

/** The refusal sound a locked button plays instead of activating. */
const REFUSAL = "hackfail";

async function only(game: GameServer, objectId: number) {
  const found = await game.entities.byTemplate(objectId);
  assert.equal(
    found.length,
    1,
    `expected exactly one eng1 object ${objectId}, got ${JSON.stringify(found.map((e) => e.name))}`,
  );
  return found[0];
}

async function liftPosition(game: GameServer): Promise<Vec3> {
  return (await only(game, GRAV_LIFT)).position;
}

function moved(a: Vec3, b: Vec3): boolean {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]) > 0.5;
}

async function lastSoundSequence(game: GameServer): Promise<number> {
  const { sounds } = await game.audio.recent();
  return sounds.length === 0 ? 0 : sounds[sounds.length - 1].sequence;
}

/**
 * Press a button by mission object id and report what the world did: whether it
 * refused (played the locked-button sound) and where the grav lift ended up.
 */
async function press(game: GameServer, objectId: number) {
  const since = await lastSoundSequence(game);
  const liftBefore = await liftPosition(game);

  const button = await only(game, objectId);
  await game.entities.sendMessage(button.id, { type: "Frob" });
  await game.step({ frames: 180 });

  const { sounds } = await game.audio.recent();
  const played = sounds
    .filter((sound) => sound.sequence > since)
    .map((sound) => sound.sample.toLowerCase());
  return {
    refused: played.includes(REFUSAL),
    liftMoved: moved(liftBefore, await liftPosition(game)),
    played,
  };
}

test(
  "eng1: a locked grav-lift button only works once its unlock trap fires",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8137),
    });
    await game.step({ frames: 10 });

    // Before: the button is PropLocked with no key, so it refuses and the lift
    // never moves.
    const locked = await press(game, GRAV_LIFT_BUTTON);
    assert.equal(
      locked.refused,
      true,
      `a locked button should refuse; played ${JSON.stringify(locked.played)}`,
    );
    assert.equal(
      locked.liftMoved,
      false,
      "a refused press must not call the lift",
    );

    // Fire the authored chain the player actually triggers: the Once Router
    // relays TurnOn over its SwitchLinks, one of which is the Unlock Trap.
    const router = await only(game, ONCE_ROUTER);
    await game.entities.sendMessage(router.id, { type: "TurnOn" });
    await game.step({ frames: 10 });

    // After: the same button now drives the lift.
    const unlocked = await press(game, GRAV_LIFT_BUTTON);
    assert.equal(
      unlocked.refused,
      false,
      "the unlocked button must not refuse",
    );
    assert.equal(
      unlocked.liftMoved,
      true,
      "after its unlock trap fires the button should call the grav lift",
    );

    // Control: a button whose own unlock trap was never fired stays locked.
    const stillLocked = await press(game, UNRELATED_LOCKED_BUTTON);
    assert.equal(
      stillLocked.refused,
      true,
      "firing one unlock trap must not unlock unrelated locked buttons",
    );

    // Control: the never-locked twin button still relays to the lift.
    const twin = await press(game, UNLOCKED_TWIN_BUTTON);
    assert.equal(twin.refused, false, "the twin button was never locked");
    assert.equal(
      twin.liftMoved,
      true,
      "the unlocked twin button should keep working",
    );

    // The unlock is real game state, not session state: it must survive a
    // save/load round trip (P$Locked is a registered Dark property).
    await game.save("trap-unlock-e2e");
    await game.load("trap-unlock-e2e");
    await game.step({ frames: 10 });

    const afterLoad = await press(game, GRAV_LIFT_BUTTON);
    assert.equal(afterLoad.refused, false, "the unlock must survive save/load");
    assert.equal(
      afterLoad.liftMoved,
      true,
      "the reloaded button should still call the grav lift",
    );

    const stillLockedAfterLoad = await press(game, UNRELATED_LOCKED_BUTTON);
    assert.equal(
      stillLockedAfterLoad.refused,
      true,
      "a still-locked button must stay locked across save/load too",
    );

    // The mirror edge: an off-edge at the trap (what rec2's inverter delivers
    // to its unlock trap during the dining ambush) locks the button again.
    const trap = await only(game, UNLOCK_TRAP);
    await game.entities.sendMessage(trap.id, { type: "TurnOff" });
    await game.step({ frames: 10 });

    // Only the refusal is asserted here: by this point the lift is still
    // travelling from the previous press, so its position is not a signal.
    const relocked = await press(game, GRAV_LIFT_BUTTON);
    assert.equal(
      relocked.refused,
      true,
      "turning the unlock trap off should lock its buttons again",
    );
  },
);
