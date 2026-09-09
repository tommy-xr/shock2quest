import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdir, writeFile } from "node:fs/promises";
import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`MFD item information is read-only and cancellable (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({mission: "debug_interactions", debugFlags: vr ? ["--vr"] : []});
    await game.step({frames: 30});
    const hypo = await game.player.spawnItem(-52);
    await game.input.trigger("ToggleUseMode");
    await game.step({frames: 5});
    const output = process.env.ASTRA_MFD_CAPTURE;
    const capture = async (name: string) => {
      if (!output) return;
      await mkdir(output, {recursive: true});
      const path = `${output}/${vr ? "vr" : "flat"}-${name}`;
      await game.screenshot(`${path}.png`, 1600);
      await writeFile(`${path}.json`, JSON.stringify({ui: await game.ui.state(), inventory: await game.player.inventory(), info: await game.info()}, null, 2));
    };
    const elements = async () => (await game.ui.state()).strip!.elements;
    const control = async (label: string) => {
      const element = (await elements()).find(e => e.label === label);
      assert.ok(element, `${label} exists`);
      return element;
    };
    // Reuse the common VR canvas aim; flat screen_rect follows the existing loot tests.
    const click = async (element: UiElement) => {
      if (vr) {
        const ui = await game.ui.state();
        const [x,y,w,h] = element.rect;
        await aimVrHandAtCanvas(game, ui.panel_pose!, [x+w/2,y+h/2], {hand:"right",squeeze:0});
        await game.input.set("right_hand.trigger",1);
      } else {
        const [x,y,w,h] = element.screen_rect;
        await game.input.set("pointer.position",[x+w/2,y+h/2]);
        await game.step({frames:2});
        await game.input.set("pointer.pressed",1);
      }
      await game.step({frames:2});
      await game.input.set(vr ? "right_hand.trigger" : "pointer.pressed",0);
      await game.step({frames:2});
    };
    const before = await game.player.inventory();
    const cursor = (await game.ui.state()).cursor;
    await capture("idle");
    await click(await control("inspect"));
    await capture("select");
    await click(await control("inspect"));
    assert.equal((await elements()).some(e => e.label === "utility_close"),false,"second ? cancels selection");
    await click(await control("inspect"));
    const item = (await elements()).find(e => e.entity_id === hypo.entity_id && e.kind === "button");
    assert.ok(item);
    await click(item);
    const text = (await elements()).filter(e => e.label === "utility_text");
    assert.ok(text.length > 0);
    assert.doesNotMatch(text.map(e=>e.text).join(" "), /No description available/i, "authored hypo description resolves");
    assert.ok(text.every(e => e.entity_id === hypo.entity_id),"description belongs to selected hypo");
    await capture("description");
    const next = (await elements()).find(e => e.label === "utility_next");
    assert.ok(next, "medical hypo description exercises pagination");
    if (next) {
      const first = text.map(e=>e.text);
      await click(next);
      assert.notDeepEqual((await elements()).filter(e=>e.label === "utility_text").map(e=>e.text),first);
      await capture("page-2");
      await click(await control("utility_previous"));
      assert.deepEqual((await elements()).filter(e=>e.label === "utility_text").map(e=>e.text),first);
    }
    await click(await control("utility_close"));
    assert.equal((await elements()).some(e=>e.label === "utility_close"),false);
    assert.deepEqual(await game.player.inventory(),before,"inspection does not consume or move hypo");
    assert.deepEqual((await game.ui.state()).cursor,cursor,"inspection leaves cursor item unchanged");
    await capture("closed");
  });
}


test("VR inspection reserves an off-panel held hypo trigger through cancel until release", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({mission:"debug_interactions",debugFlags:["--vr"]});
  await game.step({frames:30});
  const hypo = (await game.entities.list()).entities.find(e=>e.template_id === -52);
  assert.ok(hypo);
  await aimVrHandAt(game,hypo.position,.2,1,0,{hand:"right"});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id);
  await game.input.set("right_hand.position",[.8,1,-.2]);
  await game.input.set("right_hand.rotation",[0,0,0,1]);
  await game.input.set("head.rotation",[0,0,0,1]);
  await game.input.trigger("ToggleUseMode");
  await game.step({frames:5});
  const stack = (await game.entities.detail(hypo.id)).properties.find(p=>p.name === "StackCount");
  assert.ok(stack);
  const health = (await game.info()).player.hit_points;
  const inspect = (await game.ui.state()).strip!.elements.find(e=>e.label === "inspect")!;
  const [x,y,w,h] = inspect.rect;
  await aimVrHandAtCanvas(game,(await game.ui.state()).panel_pose!,[x+w/2,y+h/2],{hand:"left",squeeze:0});
  await game.input.set("left_hand.trigger",1);
  await game.step({frames:2});
  await game.input.set("left_hand.trigger",0);
  await game.step({frames:2});
  assert.ok((await game.ui.state()).strip!.elements.some(e=>e.label === "utility_close"));
  // The holding hand points outside every panel. Its trigger must still be reserved.
  await game.input.set("right_hand.rotation",[0,1,0,0]);
  await game.input.set("right_hand.trigger",1);
  await game.step({frames:5});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id);
  assert.equal((await game.info()).player.hit_points,health);
  assert.deepEqual((await game.entities.detail(hypo.id)).properties.find(p=>p.name === "StackCount"),stack,"held hypo properties/stack remain unchanged");
  // Closing the interface cancels inspection while the held trigger remains down.
  await game.input.trigger("ToggleUseMode");
  await game.step({frames:5});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id);
  assert.equal((await game.info()).player.hit_points,health);
  assert.deepEqual((await game.entities.detail(hypo.id)).properties.find(p=>p.name === "StackCount"),stack);
  await game.input.set("right_hand.trigger",0);
  await game.step({frames:3});
  assert.equal((await game.info()).player.right_hand_entity_id,hypo.id,"releasing reserved trigger does not consume hypo");
});
