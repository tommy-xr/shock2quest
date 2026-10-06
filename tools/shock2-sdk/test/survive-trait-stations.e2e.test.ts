import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type Vec3 } from "../src/index.js";
import { add, quatRotate, type Quat } from "./helpers/vr-hand.js";

// Original mission object IDs and authored payphone poses, independent of
// runtime-assigned entity IDs. First two are street, last two are subway.
const phones: [number, Vec3, Quat][] = [
  [591, [46.74778, 22.2, 19.97069], [0, 1, 0, 0]],
  [680, [28.438915, 22.2, 10.272737], [0, Math.SQRT1_2, 0, Math.SQRT1_2]],
  [1013, [-3.316, 3.2, .5249911], [0, Math.SQRT1_2, 0, -Math.SQRT1_2]],
  [1018, [21.555788, 3.2, 12.420635], [0, 0, 0, 1]],
];
const unlockWaves = [0, 3, 6, 9];

test("Survive replaces every payphone with a wall-mounted, wave-gated trait station and preserves it on load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth_horde" });
  await game.devParams.set("cheat", 1);
  await game.step({ frames: 2 });
  const check = async (wave: number) => {
    for (const [index, [phone, position, rotation]] of phones.entries()) {
      assert.equal((await game.entities.byTemplate(phone)).length, 0, `payphone ${phone} was replaced`);
      const stations = await game.entities.byTemplate(60300 + index);
      assert.equal(stations.length, 1);
      const station = stations[0];
      const detail = await game.entities.detail(station.id);
      const phoneFront = quatRotate(rotation, [0, 0, -1]);
      const stationFront = quatRotate(detail.rotation, [0, 0, 1]);
      stationFront.forEach((value, axis) =>
        assert.ok(Math.abs(value - phoneFront[axis]) < .001, "station faces the original phone's approach"));
      const expectedPosition = add(position, quatRotate(rotation, [0, 0, .25]));
      station.position.forEach((value, axis) =>
        assert.ok(Math.abs(value - expectedPosition[axis]) < .001, "shallower station is mounted toward the wall"));
      // Wall distances measured from the original pivots. A pivot inside the
      // wall is culled even when the model extends back into the room.
      const wallDistance = station.position.reduce((distance, value, axis) =>
        distance + (value - position[axis]) * phoneFront[axis], [.37069, .361085, .284, .379365][index]);
      assert.ok(wallDistance > .03, "station pivot retains room-side clearance");

      const approach = add(position, quatRotate(rotation, [0, -.5, -1.5]));
      await game.player.teleport({ x: approach[0], y: approach[1], z: approach[2] });
      await game.entities.sendMessage(station.id, { type: "Frob" });
      await game.step({ frames: 2 });
      const panel = (await game.ui.state()).active_panel;
      assert.equal(panel?.entity_id, station.id, "station is usable from the original approach");
      const offline = panel!.elements.some(e => e.text?.includes("Station offline."));
      assert.equal(offline, wave < unlockWaves[index], `station ${index} unlocks at wave ${unlockWaves[index]}`);
    }
  };
  await check(0);
  const save = `survive_trait_phones_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  await game.step({ frames: 1 });
  await check(0);
  for (const wave of [3, 6, 9]) {
    await game.devParams.set("horde_start_wave", wave);
    await game.input.trigger("DebugStartHordeWave");
    await game.step({ frames: 2 });
    await check(wave);
  }
  const unlockedSave = `survive_trait_phones_unlocked_${Date.now()}`;
  await game.save(unlockedSave);
  await game.load(unlockedSave);
  await game.step({ frames: 1 });
  await check(9);
});
