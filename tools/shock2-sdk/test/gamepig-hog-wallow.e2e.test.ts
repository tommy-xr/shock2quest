import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";
import { savedGamePath } from "./helpers/save.js";

for (const vr of [false, true]) {
  test(`Hog Wallow aims, fires, bombs and saves campaign scores (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    const { handheld, click } = await openGamePig(game, vr, "Abyss Cart");
    const elements = async () => (await game.ui.state()).active_panel!.elements;
    const control = async (label: string) => {
      const found = (await elements()).find(e => e.label === label);
      assert.ok(found, `missing playable ${label}`);
      return found;
    };
    const status = async () => (await elements()).find(e => /^L\d+ /.test(e.text ?? ""))!.text!;
    const score = async () => Number((await status()).split(" ").at(-1));
    const button = async (action: "LeftHandLowerButton" | "LeftHandUpperButton", frames = 3) => {
      await game.input.hold(action); await game.step({ frames });
      await game.input.release(action); await game.step({ frames: 3 });
    };
    const bomb = async () => {
      if (vr) await button("LeftHandUpperButton");
      else {
        const field = await control("wallow-fire");
        const [x,y,w,h] = field.screen_rect;
        await game.input.set("pointer.position", [x+w/2,y+h/2]);
        await game.input.set("pointer.secondary", 1); await game.step({ frames: 2 });
        await game.input.set("pointer.secondary", 0); await game.step({ frames: 2 });
      }
    };
    await click(game, await control("game-6"));
    await control("wallow-fire");
    const geometry = async () => (await elements()).filter(e => e.kind === "fill" && e.rect[3] < 2).map(e => e.rect.map(Math.round));
    const before = await geometry();
    assert.ok(before.length > 100, "projected tunnel is present");
    if (vr) {
      const rotation = (await game.info()).player.rotation;
      await game.input.set("left_hand.thumbstick", [0,1]); await game.step({ frames: 2 });
      await game.input.set("left_hand.thumbstick", [0,0]); await game.step({ frames: 2 });
      assert.deepEqual((await game.info()).player.rotation, rotation, "holding hand aims without turning the player");
      assert.notDeepEqual(await geometry(), before, "aiming moves the ship around the rim");
      await button("LeftHandLowerButton");
    } else {
      const field = await control("wallow-fire");
      const [x,y,w,h] = field.screen_rect;
      await click(game, { ...field, screen_rect: [x+w*0.5,y+h*0.12,0,0] });
      assert.notDeepEqual(await geometry(), before, "pointer aim and primary fire change the wireframe");
    }
    // Let the fired shot expire, then wait for visible enemy movement rather than a random spawn deadline.
    await game.step({ frames: 100 });
    let moving = false;
    for (let i=0;i<50 && !moving;i++) {
      const prior = JSON.stringify(await geometry());
      await game.step({ frames: 6 });
      moving = JSON.stringify(await geometry()) !== prior;
    }
    assert.ok(moving, "an enemy wave is visible before bombing");
    const ordinaryScore = await score();
    await bomb();
    assert.equal(await score(), ordinaryScore, "unhacked secondary cannot bomb enemies");
    await game.entities.sendMessage(handheld.entity_id, { type: "SetObjectState", state: "Hacked" });
    await game.step({ frames: 2 });
    await bomb();
    assert.ok(await score() > ordinaryScore, "hacked bomb destroys the wave and awards points");
    assert.ok((await elements()).some(e => e.text?.includes("Bomb:--")), "bomb is spent for this level");
    const bombScore = await score();
    await game.step({ frames: 60 }); await bomb();
    assert.equal(await score(), bombScore, "second bomb cannot score twice");
    for (let i=0;i<25 && !(await elements()).some(e => e.label === "wallow-restart");i++) await game.step({ frames: 600 });
    await control("wallow-restart");
    const finalScore = await score();
    const name = `wallow_${vr}_${Date.now()}`;
    try {
      await game.save(name);
      const scores = () => JSON.parse(readFileSync(savedGamePath(name),"utf8")).global_data.quest_info.mini_game_scores["6"];
      const stored = scores();
      assert.equal(stored.length,1); assert.equal(stored[0].points,finalScore);
      await game.step({ frames: 60 }); await game.save(name);
      assert.deepEqual(scores(),stored,"terminal frames record once");
      await click(game,await control("wallow-restart"));
      assert.equal(await score(),0);
      await game.load(name); await game.step({ frames: 2 }); await game.save(name);
      assert.deepEqual(scores(),stored,"same-build save/load retains campaign scores");
    } finally { rmSync(savedGamePath(name),{force:true}); }
  });
}
