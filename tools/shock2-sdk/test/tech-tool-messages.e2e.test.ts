import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";
import { add, aimVrHandAt, scale, sub } from "./helpers/vr-hand.js";

// Retail tech-cursor feedback (MISC.STR) for a held tool, identical in flat
// and VR: the bracket slot reads the verb ("Hack") while the target will take
// the tool, and the use announces "Device hacked!".
const enabled = process.env.SHOCK2_E2E === "1";

const ICE_PICK = -73;
const SECURITY_CRATE = 325; // hydro1, still hackable
const HACK = "Hack";
const HACKED = "Device hacked!";

async function nearCrate(game: GameServer) {
  await game.step({ frames: 5 });
  const [crate] = await game.entities.byTemplate(SECURITY_CRATE);
  assert.ok(crate, "hydro1 should contain security crate 325");
  const cratePos = (await game.entities.detail(crate.id)).position as Vec3;
  await teleportVerified(game, { x: cratePos[0] + 1.2, y: cratePos[1] + 0.5, z: cratePos[2] + 1.2 });
  await game.step({ frames: 3 });
  return { crate, cratePos };
}

function stripItem(ui: UiState, name: string): UiElement {
  const item = ui.strip?.elements.find((e) => e.kind === "button" && e.label === name);
  assert.ok(item, `inventory strip should contain ${name}`);
  return item;
}

async function secondaryClickBareView(game: GameServer): Promise<void> {
  await game.input.set("pointer.position", [0.5, 0.6]);
  for (const value of [0, 1, 0]) {
    await game.input.set("pointer.secondary", value);
    await game.step({ frames: 2 });
  }
}

test("flat: verb under the brackets, then the authored hack lines", { skip: !enabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "hydro1.mis" });
  const { crate } = await nearCrate(game);
  await game.player.aimAt(crate, { hitbox: "center", visibility: "required" });
  await game.step({ frames: 3 });
  await game.player.spawnItem(ICE_PICK);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });

  await clickUiElement(game, stripItem(await game.ui.state(), "ICE Pick"));
  await game.step({ frames: 2 });
  let ui = await game.ui.state();
  assert.deepEqual(ui.bracket_hints, [HACK], "the ready crate's slot reads the tech verb alone");

  await secondaryClickBareView(game);
  await game.step({ frames: 3 });
  ui = await game.ui.state();
  assert.ok(ui.messages.includes(HACKED), `messages: ${JSON.stringify(ui.messages)}`);
  assert.ok(!ui.bracket_hints.includes(HACK), "nothing is ready once the pick is spent");
});

test("VR: verb under the brackets, then the authored hack lines", { skip: !enabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "hydro1.mis", debugFlags: ["--vr"] });
  const { cratePos } = await nearCrate(game);
  const player = (await game.info()).player.position as Vec3;
  const away = add(player, scale(sub(player, cratePos), 2));

  async function holdPick() {
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.player.spawnItem(ICE_PICK, { hand: "right" });
    await game.step({ frames: 5 });
    const held = (await game.info()).player.right_hand_entity_id;
    assert.ok(held !== null, "the right hand should hold the ICE Pick");
    await aimVrHandAt(game, away, 0.45, 1);
    await game.step({ frames: 5 });
    return held;
  }
  async function release() {
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
  }

  await holdPick();
  assert.ok(!(await game.ui.state()).bracket_hints.includes(HACK), "no verb away from the crate");
  await aimVrHandAt(game, cratePos, 0.45, 1);
  assert.deepEqual((await game.ui.state()).bracket_hints, [HACK], "the ready crate's slot reads the tech verb alone");
  await release();
  let ui = await game.ui.state();
  assert.ok(ui.messages.includes(HACKED), `messages: ${JSON.stringify(ui.messages)}`);
});
