import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Every annelid egg pod in the game used to hatch into nothing: all three
// scripts (GooEgg / GrubEgg / SwarmerEgg) were mapped to the plain model tweq,
// so a pod opened its shell and produced no payload.
//
// Driven in `debug_annelid`, which places one pod of each kind and nothing
// else. A shipped level is the wrong bench here: its pods are armed with
// authored tripwires that the player's own spawn can already have sprung, so
// "did MY TurnOn hatch it" is not answerable there. The pods themselves are the
// shipped gamesys templates running the shipped scripts.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)

/** Pod template -> the gamesys template its script must create. */
const HATCHES: { pod: number; payload: number; what: string }[] = [
  // GooEgg creates two objects; assert the cloud, because the emitter destroys
  // itself the moment its four shots are away. The shots are the second test.
  { pod: -1476, payload: -438, what: "EggGooCloud" },
  { pod: -1335, payload: -182, what: "Grub" },
  { pod: -1332, payload: -183, what: "Swarm" },
];

test(
  "each annelid egg pod hatches its authored payload on TurnOn",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_annelid" });
    // The bench spawns its pods on the first stepped frame.
    await game.step({ frames: 30 });

    for (const { pod, payload, what } of HATCHES) {
      const [target] = await game.entities.byTemplate(pod);
      assert.ok(target, `the bench should place template ${pod}`);

      const before = new Set(
        (await game.entities.byTemplate(payload)).map((entity) => entity.id),
      );
      await game.entities.sendMessage(target.id, { type: "TurnOn" });
      // Enough to deliver the message and apply the hatch effects.
      await game.step({ frames: 5 });
      const hatched = (await game.entities.byTemplate(payload)).filter(
        (entity) => !before.has(entity.id),
      );

      assert.equal(hatched.length, 1, `template ${pod} should hatch a ${what}`);
    }
  },
);

test(
  "a goo pod's emitted shots survive their own volley and fly",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_annelid" });
    await game.step({ frames: 30 });
    const [pod] = await game.entities.byTemplate(-1476);
    assert.ok(pod, "the goo station should hold a Floor Pod");

    await game.entities.sendMessage(pod.id, { type: "TurnOn" });
    // The emitter releases its four shots 100 ms apart, all from one point and
    // all straight up - they used to annihilate each other at the muzzle.
    await game.step({ frames: 45 });

    const shots = await game.entities.byTemplate(-1557);
    assert.equal(shots.length, 4, "all four goo shots should still be alive");
    const heights = shots.map((shot) => shot.position[1]!);
    assert.ok(
      Math.max(...heights) - Math.min(...heights) > 0.5,
      `the volley should be strung out in flight, got ${heights.join(", ")}`,
    );

    // ...and then splat. GooProjectile owns the terminal impact (its authored
    // collision type is a plain BOUNCE, so the shared handler is inert on a
    // glob) - without it the globs never slay, and the venom is never
    // delivered to whatever they land on. They arc back down onto the pod.
    await game.step({ frames: 600 });
    assert.equal(
      (await game.entities.byTemplate(-1557)).length,
      0,
      "every goo shot should have splatted",
    );
    // grubspang is the glob's authored corpse, one per splat.
    assert.equal(
      (await game.entities.byTemplate(-2540)).length,
      4,
      "each splat should leave its authored spang",
    );
  },
);
