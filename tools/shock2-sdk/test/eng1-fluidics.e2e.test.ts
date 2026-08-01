import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable eng1 mission-object ids. Runtime ids are rediscovered every launch.
const FLUIDIC_BUTTON_OBJECT = 1114;
const FLUIDIC_COMP_OBJECT = 1152;
const OFFLINE_EMAIL_TRAP_OBJECT = 651;

// Negative-first: Fluidic Comp is authored HUDSelect(false) with PickBias
// -2000, immediately in front of the invisible HUDSelect(true) button overlay.
// Flat interaction used to stop at the computer's non-frobbable ENTITY
// collider, then filter it out, so a normal squeeze could never reach the
// overlay or fire the offline response.
test(
  "eng1: production crosshair frobs the Fluidics overlay behind its biased computer",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8194),
    });
    await game.step({ frames: 2 });

    const [button] = await game.entities.byTemplate(FLUIDIC_BUTTON_OBJECT);
    const [computer] = await game.entities.byTemplate(FLUIDIC_COMP_OBJECT);
    const [offlineEmail] = await game.entities.byTemplate(OFFLINE_EMAIL_TRAP_OBJECT);
    assert.ok(button?.name === "Fluidic Button", "expected the Fluidics button overlay");
    assert.ok(computer?.name === "Fluidic Comp", "expected the Fluidics computer");
    assert.ok(offlineEmail, "expected the one-shot offline email response");

    // Setup only: use the real campaign standing position, then drive the
    // production camera and squeeze path for the interaction under test.
    await game.player.teleport({ x: 0.956, y: -1.556, z: -175.278 });
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
    await game.step({ frames: 10 });

    assert.equal(
      (await game.entities.byTemplate(OFFLINE_EMAIL_TRAP_OBJECT)).length,
      0,
      "the production Frob should relay through the button and consume the offline email trap",
    );
    assert.equal(
      await game.quests.get("note_1_13"),
      "incomplete",
      "the offline response should grant its authored objective",
    );
  },
);
