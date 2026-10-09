import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { tagValue, describeSounds } from "./helpers/audio.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const [template, name, sample] of [
  [-928, "wrench", /^swingwr/i],
  [-24, "rapier", /^swinges/i],
  [-28, "shard", /^swingwr/i],
  [-2291, "psi sword", /^swingwr/i],
] as const) {
  test(`VR ${name} sounds once per free swing and stays quiet at rest`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const item = (await game.entities.byTemplate(template))[0]!;
    assert.ok(item);
    await aimVrHandAt(game, item.position, 0.2, 1);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, item.id);
    await game.input.set("right_hand.position", [0, 1.4, -0.5]);
    await game.step({ frames: 60 });
    const beforeSlow = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    // 0.6 u/s is below the same 2.0 u/s gate that admits contact damage.
    for (let i = 1; i <= 18; i++) {
      await game.input.set("right_hand.position", [-i * 0.01, 1.4, -0.5]);
      await game.step({ frames: 1 });
    }
    await game.step({ frames: 30 });
    assert.equal((await game.audio.recent()).sounds.filter(s => s.sequence > beforeSlow && tagValue(s, "event") === "motion").length, 0, "below-damage-speed movement must be silent");
    await game.input.set("right_hand.position", [0, 1.4, -0.5]);
    await game.step({ frames: 60 });
    const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    for (let i = 1; i <= 18; i++) {
      await game.input.set("right_hand.position", [i * 0.05, 1.4, -0.5]);
      await game.step({ frames: 1 });
    }
    const sounds = (await game.audio.recent()).sounds.filter(s => s.sequence > before && tagValue(s, "event") === "motion");
    assert.equal(sounds.length, 1, describeSounds(sounds));
    assert.match(sounds[0]!.sample, sample);
    await game.step({ frames: 60 });
    const resting = (await game.audio.recent()).sounds.filter(s => s.sequence > sounds[0]!.sequence && tagValue(s, "event") === "motion");
    assert.equal(resting.length, 0, describeSounds(resting));
    for (let i = 1; i <= 18; i++) {
      await game.input.set("right_hand.position", [0.9 - i * 0.05, 1.4, -0.5]);
      await game.step({ frames: 1 });
    }
    const again = (await game.audio.recent()).sounds.filter(s => s.sequence > sounds[0]!.sequence && tagValue(s, "event") === "motion");
    assert.equal(again.length, 1, describeSounds(again));
  });
}

test("flat melee whooshes even on a miss, without retriggering an active swing", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions" });
  await game.player.spawnItem(-928);
  await game.input.trigger("EquipWrench");
  await game.step({ frames: 5 });
  const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  const sounds = (await game.audio.recent()).sounds.filter(s => s.sequence > before && tagValue(s, "event") === "motion");
  assert.equal(sounds.length, 1, describeSounds(sounds));
});
