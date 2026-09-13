import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("VR cup throw inherits motion and deals at most two damage once to a hybrid",
  { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({
      mission: "debug_interactions", debugFlags: ["--vr"],
    });
    await game.step({ frames: 60 });
    const mug = (await game.entities.list({ filter: "Mug" })).entities.find(
      entity => entity.template_id === -1221,
    );
    assert.ok(mug, "rack contains the authored coffee mug");
    await game.input.set("right_hand.position", [-1.2, 0.05, -1.2]);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.right_hand_entity_id, mug.id);

    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 1 });
    const hybrid = (await game.entities.list({ filter: "OG-Pipe" })).entities.find(
      entity => entity.template_id === -397,
    );
    assert.ok(hybrid, "debug spawn creates a real hybrid");
    await game.input.trigger("DebugCalmAll");
    const target = await game.entities.detail(hybrid.id);
    const pawn = (await game.info()).player.position;
    const start = [target.position[0] + 1.8, target.position[1] + 0.6, target.position[2]];
    // Lift the mug clear of its pedestal into the open aisle, then stop.
    for (let frame = 1; frame <= 24; frame++) {
      const world = [
        -1.2 + (start[0] + 1.2) * frame / 24,
        1.29 + (start[1] - 1.29) * frame / 24,
        -1.2 + (start[2] + 1.2) * frame / 24,
      ];
      await game.input.set("right_hand.position", world.map((value, axis) => value - pawn[axis]));
      await game.step({ frames: 1 });
    }
    await game.step({ frames: 10 });
    const hp = async () => {
      assert.ok(hybrid);
      const detail = await game.entities.detail(hybrid.id);
      const property = detail.properties.find(property => property.name === "HitPoints");
      assert.ok(property);
      return Number(property.value);
    };
    const before = await hp();
    // Six world units/second controller motion; debug scene Strength 6 adds 25%.
    for (let frame = 1; frame <= 6; frame++) {
      await game.input.set("right_hand.position", [
        start[0] - 0.1 * frame - pawn[0], start[1] - pawn[1], start[2] - pawn[2],
      ]);
      await game.step({ frames: 1 });
    }
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    const bodies = (await game.physics.bodies({ entityId: mug.id })).bodies;
    assert.equal(bodies.length, 1, "released cup is a physical object");
    assert.ok(bodies[0].velocity[0] < -6 && bodies[0].velocity[0] > -9,
      `release carries the swing with Strength scaling: ${bodies[0].velocity}`);
    // Check the live collision path, including the rebound and later contacts.
    let minimum = before;
    for (let frame = 0; frame < 30; frame++) {
      await game.step({ frames: 2 });
      const current = await hp();
      assert.ok(current >= before - 2, "a cup never exceeds two organic damage");
      minimum = Math.min(minimum, current);
    }
    assert.equal(minimum, before - 2, "the fast cup strike reaches its low damage cap");
    assert.equal(await hp(), before - 2, "bouncing/contact persistence cannot damage again");
  });


test("saving a real mission during a cup throw preserves flight", { skip: !enabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 2 });
  const findMug = async () => {
    const mug = (await game.entities.list({ filter: "Mug" })).entities.find(entity => entity.template_id === 439);
    assert.ok(mug, "find the authored MedSci coffee cup each launch/load");
    return mug;
  };
  let mug = await findMug();
  const position = (await game.entities.detail(mug.id)).position;
  await game.player.teleport({ x: position[0], y: position[1], z: position[2] + 0.7 });
  await game.step({ frames: 2 });
  const pawn = (await game.info()).player.position;
  const local = position.map((v, i) => v - pawn[i]);
  await game.input.set("right_hand.position", local);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.right_hand_entity_id, mug.id);
  for (let frame = 1; frame <= 6; frame++) {
    await game.input.set("right_hand.position", [local[0], local[1] + 0.1 * frame, local[2]]);
    await game.step({ frames: 1 });
  }
  await game.input.set("right_hand.squeeze", 0);
  const before = (await game.physics.bodies({ entityId: mug.id })).bodies[0];
  assert.ok(before.velocity[1] > 4, "the cup is moving upward at save time");
  const save = `test_throwing_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  mug = await findMug();
  const after = (await game.physics.bodies({ entityId: mug.id })).bodies[0];
  assert.ok(Math.hypot(...after.velocity.map((v, i) => v - before.velocity[i])) < 0.001,
    "save/load preserves the incoming throw velocity");
});
