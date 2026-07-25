import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end coverage for the PictureSwap script (#587): the Rec deck's
// `Code Art` frames carry a model tweq whose frames spell out part of the
// transmitter code 14106. Each frob must advance one entry of
// `PropTweqModelConfig.model_names`, so the code frame (`code10` on rec1's
// Code Pic 1) becomes visible in-world.
//
// Negative-first: with `pictureswap` mapped to NoopScript, the frame never
// leaves its authored `pic05` model and the code cannot be read.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// rec1's two frames, identified by their authored names (runtime entity ids
// change every launch).
const CODE_PIC_1 = "Code Pic 1"; // models: pic05 static pic03 static code10 static
const CODE_PIC_3 = "Code Pic 3"; // models: pic01 static pic08 static code6 static

function modelOf(detail: { properties: { name: string; value: string }[] }): string {
  const p = detail.properties.find((x) => x.name === "Model");
  assert.ok(p, "entity should expose a Model property");
  return p.value;
}

test(
  "rec1.mis: frobbing a Code Art frame advances its model tweq",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8153),
    });
    await game.step({ frames: 5 });

    const frames = (await game.entities.list({ filter: "Code Pic", limit: 50 }))
      .entities;
    const pic1 = frames.find((e) => e.name === CODE_PIC_1);
    const pic3 = frames.find((e) => e.name === CODE_PIC_3);
    assert.ok(pic1, `expected the ${CODE_PIC_1} frame in rec1`);
    assert.ok(pic3, `expected the ${CODE_PIC_3} frame in rec1`);

    assert.equal(modelOf(await game.entities.detail(pic1.id)), "pic05");

    // The authored cycle is a bounce (no WRAP flag), so frobs walk the list
    // forward one entry at a time: the code digit is the 5th entry.
    const expected = ["static", "pic03", "static", "code10", "static"];
    for (const model of expected) {
      await game.entities.sendMessage(pic1.id, { type: "Frob" });
      await game.step({ frames: 2 });
      assert.equal(
        modelOf(await game.entities.detail(pic1.id)),
        model,
        `frob should advance the frame to ${model}`,
      );
    }

    // At the top edge, a non-wrapping tweq reverses instead of restarting, so
    // the next frob steps back toward the code frame rather than to pic05.
    await game.entities.sendMessage(pic1.id, { type: "Frob" });
    await game.step({ frames: 2 });
    assert.equal(modelOf(await game.entities.detail(pic1.id)), "code10");

    // The second rec1 frame cycles independently to its own digit.
    assert.equal(modelOf(await game.entities.detail(pic3.id)), "pic01");
    for (let i = 0; i < 4; i++) {
      await game.entities.sendMessage(pic3.id, { type: "Frob" });
      await game.step({ frames: 2 });
    }
    assert.equal(modelOf(await game.entities.detail(pic3.id)), "code6");
  },
);
