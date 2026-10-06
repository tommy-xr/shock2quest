import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// Use a real world-frob target. Download pickups such as nanites deliberately
// ignore the VR trigger: their interaction is physical grab and release.
const enabled = process.env.SHOCK2_E2E === "1";
const KEYPAD = 1681;

async function stage(game: GameServer) {
  await game.step({ frames: 30 });
  const [keypad] = await game.entities.byTemplate(KEYPAD);
  assert.ok(keypad, "the authored MedSci cryo-exit keypad");
  const [x, y, z] = keypad.position;
  await game.player.teleport({ x: x + 1, y: y + 0.5, z: z + 1 });
  await game.step({ frames: 30 });
  // Put the interface behind the interaction ray, with a real standing pawn.
  await game.input.set("head.look", [180, 0]);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  assert.equal((await game.ui.state()).mode, "use");
  assert.equal((await game.ui.state()).active_panel, null);
  return keypad;
}

for (const startOnPanel of [false, true]) {
  test(startOnPanel
    ? "a trigger pull begun on the cyber-interface panel cannot slide off into a world frob"
    : "an off-panel hand still frobs the world while the VR cyber interface is open", {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
    const keypad = await stage(game);
    const sequence = (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
    const frobs = async () => (await game.messages.recent()).messages.filter(message =>
      message.sequence > sequence && message.to.entity_id === keypad.id && message.payload === "Frob");
    if (startOnPanel) {
      const panel = (await game.ui.state()).panel_pose;
      assert.ok(panel);
      await aimVrHandAtCanvas(game, panel, [320, 240], { trigger: 1 });
      await game.step({ frames: 5 });
      assert.ok((await game.ui.state()).pointer?.hand, "the pull begins on the canvas");
    }
    await aimVrHandAt(game, keypad.position, 0.45, 0, startOnPanel ? 1 : 0);
    await game.step({ frames: 5 });
    assert.equal((await game.ui.state()).pointer?.hand ?? null, null, "the ray is off-panel");
    assert.equal((await frobs()).length, 0, "a panel pull cannot become a world frob");
    assert.equal((await game.ui.state()).active_panel, null);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
    assert.equal((await frobs()).length, 1, "one fresh off-panel pull reaches the keypad");
    assert.equal((await game.ui.state()).active_panel?.name, "Keypad", "the world frob opens its real panel");
    assert.equal((await game.ui.state()).mode, "use", "world interaction leaves the interface open");
  });
}
