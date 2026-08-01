import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable eng1 mission-object ids. Runtime ids are rediscovered every launch.
const MASTER_POWER_BUTTON_OBJECT = 1870;
const MASTER_POWER_COMP_OBJECT = 1103;
const MASTER_POWER_ROUTER_OBJECT = 878;
const ELEVATOR_DOOR_CONTROL_OBJECT = 990;

async function lastSoundSequence(game: GameServer): Promise<number> {
  const { sounds } = await game.audio.recent();
  return sounds.length === 0 ? 0 : sounds[sounds.length - 1].sequence;
}

async function pressRefused(game: GameServer, entityId: number): Promise<boolean> {
  const since = await lastSoundSequence(game);
  await game.entities.sendMessage(entityId, { type: "Frob" });
  await game.step({ frames: 5 });
  const { sounds } = await game.audio.recent();
  return sounds.some(
    ({ sequence, sample }) =>
      sequence > since && sample.toLowerCase() === "hackfail",
  );
}

// Negative-first: Master Power Comp is authored HUDSelect(false) with empty
// FrobInfo immediately in front of an invisible HUDSelect(true) button overlay.
// Unlike Fluidic Comp it has no negative PickBias, so the flat crosshair used to
// stop on the decorative shell and never reach the production button.
test(
  "eng1: production crosshair frobs the Master Power overlay behind its non-selectable computer",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8195),
    });
    await game.step({ frames: 2 });

    const [button] = await game.entities.byTemplate(MASTER_POWER_BUTTON_OBJECT);
    const [computer] = await game.entities.byTemplate(MASTER_POWER_COMP_OBJECT);
    const [router] = await game.entities.byTemplate(MASTER_POWER_ROUTER_OBJECT);
    const [elevatorControl] = await game.entities.byTemplate(
      ELEVATOR_DOOR_CONTROL_OBJECT,
    );
    assert.ok(button?.name === "Master Power Button", "expected the button overlay");
    assert.ok(computer?.name === "Master Power Comp", "expected the computer shell");
    assert.ok(router?.name === "Once Router", "expected the one-shot success router");
    assert.ok(
      elevatorControl?.name === "Elevator Button",
      "expected the locked elevator control",
    );

    // Setup only: satisfy the authored nacelle filter and stand at the exact
    // campaign position, then drive the production camera and squeeze path.
    await game.quests.set("NacellesFrobbed", "incomplete");
    assert.equal(
      await pressRefused(game, elevatorControl.id),
      true,
      "the authored elevator control should begin locked",
    );
    await game.player.teleport({ x: 1.5, y: 4.444, z: -19.72 });
    await game.step({ frames: 2 });
    const aim = await game.player.aimAt(button, { hitbox: "center" });
    assert.equal(
      aim.interaction_target_id,
      computer.id,
      "the unprioritized combined ray should document the computer occluder",
    );
    assert.equal(aim.target_confirmed, false);

    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 30 });

    const [poweredComputer] = await game.entities.byTemplate(
      MASTER_POWER_COMP_OBJECT,
    );
    assert.ok(poweredComputer, "the Master Power computer should remain in the mission");
    assert.equal(
      (await game.entities.detail(poweredComputer.id)).properties.find(
        ({ name }) => name === "Model",
      )?.value,
      "engon",
      "the production Frob should switch the Master Power display on",
    );
    assert.equal(await game.quests.get("CorePower"), "incomplete");
    assert.equal(await game.quests.get("note_1_5"), "complete");
    assert.equal(await game.quests.get("note_1_1"), "complete");
    assert.equal(await game.quests.get("ElevState"), "incomplete");
    assert.equal(
      (await game.entities.byTemplate(MASTER_POWER_ROUTER_OBJECT)).length,
      0,
      "the successful production Frob should consume the one-shot power router",
    );
    assert.equal(
      await pressRefused(game, elevatorControl.id),
      false,
      "restoring Master Power should unlock the elevator control",
    );
  },
);
