import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// Strength is load-bearing inventory state: the usable backpack width changes,
// `Contains` ordinals must be re-encoded to keep (x,y), and the shipped BLOCK
// art must cover every unavailable cell in both presentations.
//
// Negative-first: main exposes all 15 columns at baseline Strength 1, draws no
// BLOCK art, and stores the four column-major items at 0,15,30,1 rather than
// the Strength-1 width's 0,10,20,1.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8608);

const isBlock = (element: UiElement) =>
  element.kind === "image" &&
  (element.texture ?? "").toLowerCase() === "iface/block.pcx";

async function backpackOrdinals(game: GameServer): Promise<number[]> {
  const info = await game.info();
  const backpackId = info.player.inventory_entity_id;
  assert.ok(backpackId != null, "mission exposes the live backpack entity");
  const backpack = await game.entities.detail(backpackId);
  return backpack.outgoing_links
    .filter((link) => link.link_type.startsWith("Contains"))
    .map((link) => link.contains_ordinal)
    .filter((slot): slot is number => slot !== null && slot !== undefined)
    .sort((a, b) => a - b);
}

async function clickElement(game: GameServer, element: UiElement) {
  const [x, y, width, height] = element.screen_rect;
  await game.input.set("pointer.position", [x + width / 2, y + height / 2]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

async function standNear(game: GameServer, id: number) {
  const [x, y, z] = (await game.entities.detail(id)).position;
  for (const [dx, dz] of [
    [0, 1.2],
    [0, -1.2],
    [1.2, 0],
    [-1.2, 0],
  ]) {
    await teleportVerified(game, { x: x + dx, y: y + 0.5, z: z + dz });
    await game.step({ frames: 30 });
    const player = (await game.info()).player.position;
    if (Math.hypot(player[0] - x, player[1] - y, player[2] - z) < 3) return;
  }
  assert.fail("could not find stable footing near the trait machine");
}

test(
  "Strength grows the backpack without moving items and ordinals survive save/load",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `inventory_strength_${Date.now()}`;
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });
    await game.step({ frames: 5 });

    assert.equal((await game.info()).player.stats?.strength, 1);
    const items = await Promise.all([
      game.player.spawnItem("Nanites"),
      game.player.spawnItem("Nanites"),
      game.player.spawnItem("Nanites"),
      game.player.spawnItem("Nanites"),
    ]);
    assert.deepEqual(
      await backpackOrdinals(game),
      [0, 1, 10, 20],
      "Strength-1 insertion uses the ten-column encoding",
    );

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const before = await game.ui.state();
    assert.ok(before.strip, "use mode exposes the backpack strip");
    assert.equal(
      before.strip.elements.filter(isBlock).length,
      15,
      "Strength 1 covers five columns x three rows",
    );
    const itemRects = items.map((item) =>
      before.strip!.elements.find((element) => element.entity_id === item.entity_id)?.rect,
    );
    assert.equal(
      itemRects.filter((rect) => rect !== undefined).length,
      4,
      "all four provisioned items render in the strip",
    );

    const raised = await game.player.setStats({ strength: 2 });
    assert.equal(raised.strength, 2);
    await game.step({ frames: 3 });
    assert.deepEqual(
      await backpackOrdinals(game),
      [0, 1, 11, 22],
      "row ordinals re-encode for width 11",
    );
    const after = await game.ui.state();
    assert.ok(after.strip);
    assert.equal(
      after.strip.elements.filter(isBlock).length,
      12,
      "Strength 2 uncovers exactly one three-cell column",
    );
    for (const [index, item] of items.entries()) {
      const afterRect: UiElement["rect"] | undefined = after.strip.elements.find(
        (element) => element.entity_id === item.entity_id,
      )?.rect;
      assert.deepEqual(afterRect, itemRects[index], "the icon keeps its (x,y)");
    }

    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.stats?.strength, 2);
    assert.deepEqual(
      await backpackOrdinals(game),
      [0, 1, 11, 22],
      "save/load preserves the width-adjusted ordinals",
    );
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
    const loaded = await game.ui.state();
    assert.ok(loaded.strip);
    assert.equal(loaded.strip.elements.filter(isBlock).length, 12);
  },
);

test(
  "VR backpack uses the same Strength-1 blocked-cell layout",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });
    await game.input.set("head.look", [0, 0]);
    // The cyber-interface use mode presents the same strip canvas in VR
    // (the old MoveInventory world-quad backpack is gone).
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    const ui = await game.ui.state();
    assert.ok(ui.strip, "use mode binds the backpack strip in VR too");
    assert.equal(
      ui.strip.elements.filter(isBlock).length,
      15,
      "VR consumes the same five blocked columns x three rows",
    );
    // Strip rects are reported on the shared 640x480 canvas: the strip docks
    // at (2, 0), so panel-local coordinates shift right by 2.
    const blockRects = ui.strip.elements.filter(isBlock).map((element) => element.rect);
    assert.deepEqual(blockRects[0], [356, 17, 34, 32]);
    const last = blockRects.at(-1);
    assert.ok(last);
    assert.ok(Math.abs(last[0] - 496) < 0.001, `last block x: ${last[0]}`);
    assert.deepEqual(last.slice(1), [85, 34, 32]);
  },
);

test(
  "Pack-Rat is a live trait and adds one three-cell backpack column",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `inventory_pack_rat_${Date.now()}`;
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      port: basePort + 2,
    });
    await game.step({ frames: 5 });
    await Promise.all([
      game.player.spawnItem("Nanites"),
      game.player.spawnItem("Nanites"),
      game.player.spawnItem("Nanites"),
    ]);
    assert.deepEqual(await backpackOrdinals(game), [0, 10, 20]);

    const machines = (await game.entities.list({ filter: "Trait Machine" })).entities;
    const machine = machines.find((entity) => entity.template_id === 133);
    assert.ok(machine, "medsci2 exposes its authored Trait Machine");
    await standNear(game, machine.id);
    await game.entities.sendMessage(machine.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const traitPanel = (await game.ui.state()).active_panel;
    assert.ok(traitPanel, "frobbing the machine opens its authored panel");
    const packRat = traitPanel.elements.find(
      (element) => element.kind === "button" && element.label === "Pack-Rat",
    );
    assert.ok(packRat, "Pack-Rat is offered by the authored machine");
    await clickElement(game, packRat);
    await game.step({ frames: 3 });

    assert.deepEqual((await game.info()).player.stats?.os_traits, [3]);
    assert.deepEqual(
      await backpackOrdinals(game),
      [0, 11, 22],
      "Pack-Rat re-encodes the existing rows for effective Strength 2",
    );
    // A machine MFD owns use mode without exposing the top strip. The first
    // toggle exits that MFD; the second opens ordinary inventory use mode.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
    const acquired = await game.ui.state();
    assert.ok(acquired.strip);
    assert.equal(
      acquired.strip.elements.filter(isBlock).length,
      12,
      "Pack-Rat uncovers one full three-cell column",
    );

    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    assert.deepEqual((await game.info()).player.stats?.os_traits, [3]);
    assert.deepEqual(await backpackOrdinals(game), [0, 11, 22]);
  },
);
