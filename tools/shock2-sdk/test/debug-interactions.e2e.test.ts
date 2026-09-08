import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
// Owner-approved rack contract; extend alongside INTERACTION_FIXTURES in
// shock2vr/src/scenes/debug_interactions.rs when adding another fixture.
const templates = [-1221, -1255, -4286, -928, -17, -19, -26, -27, -1358, -247];

test(
  "interaction rack: every fixture renders and can be held by either VR hand",
  { skip: !enabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_interactions",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 90 });
    const initial = (await game.entities.list()).entities;
    for (const hand of ["left", "right"] as const) {
      // Start each hand's pass at the rack, not at a pickup rolling on the
      // floor after the other hand dropped it. Runtime IDs must be rediscovered.
      if (hand === "right") {
        await game.input.trigger("DebugReloadLevel");
        await game.step({ frames: 90 });
      }
      for (const template of templates) {
        const matches = (await game.entities.list()).entities.filter(
          (e) => e.template_id === template,
        );
        assert.equal(matches.length, 1, `one fixture for ${template}`);
        const item = matches[0];
        assert.ok(
          (await game.scene.objects({ entityId: item.id })).objects.length > 0,
          `${item.name} renders`,
        );
        await game.player.teleport({ x: item.position[0], y: 1.0, z: 0 });
        await aimVrHandAt(game, item.position, 0.2, 0, 0, { hand });
        await game.step({ frames: 2 });
        await game.input.set(`${hand}_hand.squeeze`, 1);
        await game.step({ frames: 3 });
        const heldSlot = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
        assert.equal(
          (await game.info()).player[heldSlot], item.id,
          `${hand} grabs ${item.name}`,
        );
        await game.input.set(`${hand}_hand.squeeze`, 0);
        await game.step({ frames: 3 });
        assert.equal(
          (await game.info()).player[heldSlot], null,
          `${hand} releases ${item.name}`,
        );
      }
    }
    await game.input.trigger("DebugReloadLevel");
    await game.step({ frames: 90 });
    const moved = (await game.entities.list()).entities;
    await game.player.give(moved.find((e) => e.template_id === -1255)!.id);
    const mug = moved.find((e) => e.template_id === -1221)!;
    await aimVrHandAt(game, mug.position, 0.2, 1);
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.right_hand_entity_id, mug.id);
    // Hold away from the fresh rack so a still-squeezed controller cannot
    // immediately acquire another item after the scene is replaced.
    await game.input.set("right_hand.position", [0, 5, 0]);
    await game.step({ frames: 2 });
    await game.input.trigger("DebugReloadLevel");
    await game.step({ frames: 90 });
    const player = (await game.info()).player;
    assert.equal(player.wielded_entity_id, null);
    assert.equal(player.right_hand_entity_id, null);
    assert.equal((await game.player.inventory()).count, 0, "reset discards carried fixtures");
    const reset = (await game.entities.list()).entities;
    for (const template of templates) {
      const matches = reset.filter((e) => e.template_id === template);
      assert.equal(matches.length, 1, `reload restores exactly one ${template}`);
      const original = initial.find((e) => e.template_id === template)!;
      assert.ok(
        matches[0].position.every((value, axis) => Math.abs(value - original.position[axis]) < 0.001),
        `${original.name} returns to its station`,
      );
    }
  }
);
