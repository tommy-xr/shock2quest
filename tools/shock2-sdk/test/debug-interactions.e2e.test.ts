import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
// Owner-approved rack contract; extend alongside INTERACTION_FIXTURES in
// shock2vr/src/scenes/debug_interactions.rs when adding another fixture.
const templates = [-1221, -1255, -4286, -928, -17, -19, -26, -27, -1358, -247, -52, -57, -54, -53, -2949, -1488, -74, -157, -2998, -2594, -101, -102, -103, -104, -969, -1344, -1661, -106, -762, -1334, -1660, -48, -1264, -3864, -73];

test(
  "interaction rack: every fixture renders and either hand can hold samples or collect credentials",
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
        if ([-2998, -2594].includes(template)) {
          assert.equal((await game.info()).player[heldSlot], null, "credentials are collected, not held");
          assert.ok(!(await game.entities.list()).entities.some(e => e.id === item.id), "collecting consumes the card pickup");
          await game.input.set(`${hand}_hand.squeeze`, 0);
          await game.step({ frames: 3 });
          continue;
        }
        assert.equal(
          (await game.info()).player[heldSlot], item.id,
          `${hand} grabs ${item.name}`,
        );
        if ([-1221, -1255, -4286, -52, -57, -54, -53, -2949, -1488, -74, -157, -101, -102, -103, -104, -969, -1344, -1661, -106, -762, -1334, -1660, -48, -1264, -3864, -73].includes(template)) {
          const before = (await game.info()).player.hand_grips.find(g => g.hand === hand)!;
          assert.equal(before.source, "prepared", "gameplay reads a bake instead of running the search");
          assert.ok(before.grip);
          if (!before.authored) assert.ok(before.grip.contacts.filter(Boolean).length >= 3, "at least three fingers support each automatic fixture fit");
          assert.ok(before.grip.curls.every(c => Number.isFinite(c) && c >= 0 && c <= 1));
          await game.input.set(`${hand}_hand.position`, [0, 3, 0]);
          await game.input.set(`${hand}_hand.rotation`, [0, Math.sin(0.3), 0, Math.cos(0.3)]);
          await game.step({frames: 30});
          const snapshot = await game.info();
          const after = snapshot.player.hand_grips.find(g => g.hand === hand)!;
          assert.deepEqual(after, before, "tracked motion must not refit or change the cached grip");
          const offset = before.grip.offset;
          const input = { position: [0, 3, 0] as [number, number, number],
            rotation: [0, Math.sin(0.3), 0, Math.cos(0.3)] as [number, number, number, number] };
          const expected = add(snapshot.player.position, quatRotate(snapshot.player.rotation,
            add(input.position, quatRotate(input.rotation, [offset.x, offset.y, offset.z]))));
          const actual = (await game.entities.list()).entities.find(e => e.id === item.id)!.position;
          assert.ok(actual.every((v,i) => Math.abs(v-expected[i]) < 0.001), "item origin follows the fitted hand-local offset");
        }
        await game.input.set(`${hand}_hand.squeeze`, 0);
        await game.step({ frames: 3 });
        assert.equal(
          (await game.info()).player[heldSlot], null,
          `${hand} releases ${item.name}`,
        );
        assert.ok(!(await game.info()).player.hand_grips.some(g => g.hand === hand), "release clears the fitted grip");
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
