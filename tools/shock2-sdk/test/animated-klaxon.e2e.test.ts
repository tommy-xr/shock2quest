import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

test(
  "medsci1: authored klaxon alternates, plays once per bright edge, and saves its phase",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8596),
    });
    const findLight = async () => {
      const [light] = await game.entities.byTemplate(1711);
      assert.ok(light, "expected authored red klaxon 1711");
      return light;
    };
    let light = await findLight();
    const intensity = async () =>
      (await game.entities.detail(light.id)).properties.find(
        (entry) => entry.name === "AnimLightIntensity",
      )?.value;
    const klaxons = async () =>
      (await game.audio.recent()).sounds.filter(
        (sound) =>
          sound.sample === "clax1hi" &&
          sound.position.every((value, axis) => Math.abs(value - light.position[axis]) < 0.01),
      );

    await game.step({ frames: 1 });
    assert.equal(await intensity(), "0.000");
    assert.equal((await klaxons()).length, 0);
    await game.step({ frames: 30 });
    assert.equal(await intensity(), "1.000");
    const first = await klaxons();
    assert.equal(first.length, 1);
    assert.ok(first[0].tags.some(([tag, value]) => tag === "lighttype" && value === "medclax"));
    await game.step({ frames: 9 });
    assert.equal((await klaxons()).length, 1, "holding the bright phase must not replay audio");

    const saveName = "animated-klaxon-phase";
    assert.equal((await game.save(saveName)).success, true);
    const trace = async () => {
      const values = [];
      for (let i = 0; i < 12; i++) {
        await game.step({ frames: 5 });
        values.push(await intensity());
      }
      return values;
    };
    const expected = await trace();
    assert.ok(expected.includes("0.000") && expected.includes("1.000"));
    assert.equal((await klaxons()).length, 2, "the next cycle emits exactly one more klaxon");
    assert.equal((await game.load(saveName)).success, true);
    light = await findLight();
    assert.deepEqual(await trace(), expected, "save/load must retain the remaining phase time");

    await game.entities.sendMessage(light.id, { type: "TurnOff" });
    await game.step({ frames: 1 });
    assert.equal(await intensity(), "0.000");
    const count = (await klaxons()).length;
    await game.step({ frames: 90 });
    assert.equal(await intensity(), "0.000");
    assert.equal((await klaxons()).length, count, "a switched-off klaxon must stop cycling");
    await game.entities.sendMessage(light.id, { type: "TurnOn" });
    await game.step({ frames: 2 });
    assert.equal(await intensity(), "1.000");
    await game.step({ frames: 35 });
    assert.equal(await intensity(), "0.000", "switching back on must resume cycling");
  },
);
