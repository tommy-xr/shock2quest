import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { cycleToWeapon, fireOnce } from "./helpers/weapon.js";

// Firing wears a gun down: each shot costs the condition points the gun's
// reliability authors (the pistol loses 1 of its 100 per shot), in flatscreen
// and in VR alike - both presentations pull the same trigger into the same
// shared fire path.
//
// Negative-first: on the parent the runtime does not parse gun reliability at
// all, nothing decrements condition, and `/v1/info` has no
// `wielded_gun_condition` field - so every assertion below fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The pistol: condition 100, 1 point lost per shot. */
const PISTOL = -17;
const PISTOL_DEGRADE_PER_SHOT = 1.0;

async function condition(game: GameServer): Promise<number> {
  const value = (await game.info()).player.wielded_gun_condition;
  assert.equal(
    typeof value,
    "number",
    "a wielded gun must report its condition",
  );
  return value as number;
}

test(
  "each flatscreen shot wears the gun down by its authored degrade rate",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 30 });

    // The pistol is the first weapon DebugCycleWeapon hands out, and flat mode
    // wields what it spawns.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 10 });

    assert.equal(await condition(game), 100, "a shipped gun starts pristine");

    const shots = 3;
    for (let i = 0; i < shots; i += 1) await fireOnce(game);

    assert.equal(
      await condition(game),
      100 - shots * PISTOL_DEGRADE_PER_SHOT,
      "every shot costs the gun its authored condition points",
    );
  },
);

test(
  "a VR trigger pull wears the gun down the same way",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // In VR nothing is wielded until a hand grabs it.
    const weapon = await cycleToWeapon(game, (e) => e.template_id === PISTOL);
    await aimVrHandAt(game, weapon.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      weapon.id,
      "the VR right hand must hold the pistol",
    );

    assert.equal(await condition(game), 100, "a shipped gun starts pristine");

    const shots = 2;
    for (let i = 0; i < shots; i += 1) await fireOnce(game);

    assert.equal(
      await condition(game),
      100 - shots * PISTOL_DEGRADE_PER_SHOT,
      "the VR trigger reaches the same shared fire path",
    );
  },
);
