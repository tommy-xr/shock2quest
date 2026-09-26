import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

for (const presentation of ["flat", "vr"] as const) {
  test(
    `${presentation}: collecting an audio log posts a transient named HUD line and one beep`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "earth.mis",
        debugFlags: presentation === "vr" ? ["--vr"] : [],
      });
      await game.step({ frames: 5 });

      const [disc] = await game.entities.byTemplate(301);
      assert.ok(disc, "Earth training log must be present");
      const before = await game.audio.recent();
      const since = before.sounds.at(-1)?.sequence ?? 0;

      await game.entities.sendMessage(disc.id, { type: "Frob" });
      await game.step({ frames: 5 });

      const messages = (await game.ui.state()).messages;
      assert.ok(
        messages.some((message) => message.startsWith("Log ") && message.includes("TRAINER") && message.endsWith("added to PDA.")),
        `${presentation}: expected a localized pickup line, got ${JSON.stringify(messages)}`,
      );
      const sounds = (await game.audio.recent()).sounds.filter((sound) => sound.sequence > since);
      assert.equal(
        sounds.filter((sound) => sound.sample.toLowerCase().includes("linebeep")).length,
        1,
        `${presentation}: exactly one HUD posting beep`,
      );
      assert.ok(
        !sounds.some((sound) => sound.sample.toLowerCase() === "pickup"),
        `${presentation}: SCRIPT-only log must not play the MOVE item cue`,
      );

      await game.step({ frames: 300 });
      assert.ok(
        !(await game.ui.state()).messages.some((message) => message.startsWith("Log ")),
        `${presentation}: line should expire after five seconds`,
      );
    },
  );
}

test(
  "flat: an ordinary MOVE pickup posts the localized item line, then plays the distinct item cue",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 5 });
    const [clip] = await game.entities.byTemplate(249);
    assert.ok(clip, "Earth standard clip must be present");
    const before = await game.audio.recent();
    const since = before.sounds.at(-1)?.sequence ?? 0;

    await earthWorldUse(game, clip);
    await game.step({ frames: 3 });

    const messages = (await game.ui.state()).messages;
    assert.ok(
      messages.some((message) => message.toLowerCase().includes("standard bullets picked up.")),
      `expected the authored pickup line, got ${JSON.stringify(messages)}`,
    );
    const sounds = (await game.audio.recent()).sounds
      .filter((sound) => sound.sequence > since)
      .map((sound) => sound.sample.toLowerCase());
    assert.deepEqual(
      sounds.filter((sample) => sample === "linebeep" || sample === "pickup"),
      ["linebeep", "pickup"],
      "the HUD beep must precede exactly one distinct item cue",
    );
  },
);

test(
  "vr: a world grab uses the same pickup message and sound order",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const [clip] = await game.entities.byTemplate(249);
    assert.ok(clip, "Earth standard clip must be present");
    const [x, y, z] = clip.position;
    await game.player.teleport({ x: x + 1.2, y: y - 0.8, z: z + 1.2 });
    await game.step({ frames: 30 });
    const aim = await game.player.aimAt(clip.id, { hitbox: "center", visibility: "required" });
    assert.equal(aim.target_confirmed, true, "the production hand must see the clip");
    const before = await game.audio.recent();
    const since = before.sounds.at(-1)?.sequence ?? 0;

    await aimVrHandAt(game, aim.world_point, 0.35, 1);
    await game.step({ frames: 5 });

    assert.equal((await game.info()).player.right_hand_entity_id, clip.id);
    assert.ok(
      (await game.ui.state()).messages.some((message) =>
        message.toLowerCase().includes("standard bullets picked up."),
      ),
      "the VR world-grab path should post the same localized pickup line",
    );
    const sounds = (await game.audio.recent()).sounds
      .filter((sound) => sound.sequence > since)
      .map((sound) => sound.sample.toLowerCase());
    assert.deepEqual(
      sounds.filter((sample) => sample === "linebeep" || sample === "pickup"),
      ["linebeep", "pickup"],
    );
  },
);

test(
  "three scripted lines posted in one instant make three independent linebeeps",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "eng2.mis" });
    await game.step({ frames: 5 });
    const [trap] = await game.entities.byTemplate(689);
    assert.ok(trap, "eng2 authored Message Trap 689 must be present");
    const before = await game.audio.recent();
    const since = before.sounds.at(-1)?.sequence ?? 0;

    for (let i = 0; i < 3; i += 1) {
      await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    }
    await game.step({ frames: 1 });

    const messages = (await game.ui.state()).messages;
    assert.equal(messages.length, 3, `each post must remain visible: ${JSON.stringify(messages)}`);
    assert.equal(new Set(messages).size, 1, "same text is not deduplicated");
    const beeps = (await game.audio.recent()).sounds.filter(
      (sound) => sound.sequence > since && sound.sample.toLowerCase().includes("linebeep"),
    );
    assert.equal(beeps.length, 3, "each posted line must play its own beep");
  },
);

test(
  "an authored EXP trap posts the localized cyber-module gain through the same line",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 5 });
    const traps = (await game.entities.list({ filter: "Experience Trap" })).entities;
    let award: number | null = null;
    let trapId: number | null = null;
    for (const trap of traps) {
      const detail = await game.entities.detail(trap.id);
      const exp = detail.properties.find((property) => property.name === "Exp");
      if (exp && Number(exp.value) > 0) {
        award = Number(exp.value);
        trapId = trap.id;
        break;
      }
    }
    assert.ok(award !== null && trapId !== null, "MedSci1 needs an authored positive EXP trap");
    const before = await game.audio.recent();
    const since = before.sounds.at(-1)?.sequence ?? 0;

    await game.entities.sendMessage(trapId, { type: "TurnOn" });
    await game.step({ frames: 3 });

    assert.ok(
      (await game.ui.state()).messages.includes(`${award} cyber modules received.`),
      "the shipped AddExp string should be visible to the player",
    );
    assert.equal(
      (await game.audio.recent()).sounds.filter(
        (sound) => sound.sequence > since && sound.sample.toLowerCase() === "linebeep",
      ).length,
      1,
      "the award has one message and one beep",
    );
  },
);
