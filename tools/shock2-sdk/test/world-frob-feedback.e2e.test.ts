import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for retail ShockDoFrob's world-frob feedback: before an
// object's own frob runs, the player hears its `P$FrobSound` and sees its
// `P$UseMsg` on the status line.
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: only TrapMessage showed `P$UseMsg`, and `P$FrobSound` was
// not parsed, so a frobbed trainer said nothing and a Code Art picture
// changed in silence.
//
// Both objects are frobbed through the real flat crosshair and squeeze input,
// not an injected script message - the feedback belongs to the player's frob.
// Stable mission object ids, reported as `template_id`:
//   medsci1 Psi Trainer 1355: P$UseMsg "Psi1" -> USEMSG.STR
//   rec1 Code Pic 1 79: inherits P$FrobSound "ART_STATIC" from Code Art
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function worldFrob(
  game: GameServer,
  templateId: number,
  stand: { x: number; y: number; z: number },
): Promise<void> {
  const [target] = await game.entities.byTemplate(templateId);
  assert.ok(target, `expected mission object ${templateId}`);
  await game.player.teleport(stand);
  await game.step({ frames: 30 });
  await game.player.aimAt(target.id, { hitbox: "center", visibility: "required" });
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 5 });
}

test(
  "medsci1: frobbing the Psi trainer shows its UseMsg",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 10 });

    await worldFrob(game, 1355, { x: -18.9, y: -6.0, z: -51.0 });

    assert.ok(
      (await game.ui.state()).messages.includes(
        "Cybernetic Upgrade Unit for Psionic Projection",
      ),
      "the trainer's P$UseMsg key should resolve against USEMSG.STR",
    );
  },
);

test(
  "rec1: frobbing a Code Art picture plays its ART_STATIC frob sound",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "rec1.mis" });
    await game.step({ frames: 10 });
    const before = Math.max(0, ...(await game.audio.recent()).sounds.map(s => s.sequence));

    await worldFrob(game, 79, { x: -6.2, y: -3.5, z: -158.7 });

    const statics = (await game.audio.recent({ sample: "artstat" })).sounds.filter(
      s => s.sequence > before,
    );
    assert.equal(statics.length, 1, "one frob plays one ART_STATIC sample");
  },
);
