import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const LIGHT_BUTTON_OBJECT = 77;
const ANIM_OMNI_LIGHT_OBJECT = 714;
const AUX_POWER_BREAKER_OBJECT = 807;
const DEAD_POWER_CELL_OBJECT = 808;
const RECHARGING_STATION_OBJECT = 809;
const POWER_CELL_CORPSE_OBJECT = 107;

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function property(
  detail: { properties: { name: string; value: string }[] },
  name: string,
): string | undefined {
  return detail.properties.find((entry) => entry.name === name)?.value;
}

test(
  "rec1: the auxiliary-power switch restores its authored animated lights",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
    });
    await game.step({ frames: 5 });

    // Runtime ids are deliberately not stable. The concrete mission object ids
    // are: Light_button 77 -> 17 SwitchLinks, including Anim Omni Light 714.
    const [button] = await game.entities.byTemplate(LIGHT_BUTTON_OBJECT);
    const [omni] = await game.entities.byTemplate(ANIM_OMNI_LIGHT_OBJECT);
    const [breaker] = await game.entities.byTemplate(AUX_POWER_BREAKER_OBJECT);
    const [deadCell] = await game.entities.byTemplate(DEAD_POWER_CELL_OBJECT);
    const [station] = await game.entities.byTemplate(RECHARGING_STATION_OBJECT);
    const [corpse] = await game.entities.byTemplate(POWER_CELL_CORPSE_OBJECT);
    assert.ok(button, `expected rec1 object ${LIGHT_BUTTON_OBJECT}`);
    assert.ok(omni, `expected rec1 object ${ANIM_OMNI_LIGHT_OBJECT}`);
    assert.ok(breaker, `expected rec1 object ${AUX_POWER_BREAKER_OBJECT}`);
    assert.ok(deadCell, `expected rec1 object ${DEAD_POWER_CELL_OBJECT}`);
    assert.ok(station, `expected rec1 object ${RECHARGING_STATION_OBJECT}`);
    assert.ok(corpse, `expected rec1 object ${POWER_CELL_CORPSE_OBJECT}`);

    // Exercise the authentic production setup: pick up rec1's authored dead
    // cell from the REC Male Corpse's loot MFD, then frob its recharging
    // station, leaving a charged Power Cell in the backpack for the Aux Power
    // receptor to consume below.
    await game.player.teleport({
      x: corpse.position[0] + 1,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1,
    });
    await game.entities.sendMessage(corpse.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const corpsePanel = (await game.ui.state()).active_panel;
    assert.ok(corpsePanel, "frobbing the REC Male Corpse must open its loot MFD");
    const cellElement = corpsePanel.elements.find(
      (element) =>
        element.kind === "button" && element.entity_id === deadCell.id,
    );
    assert.ok(cellElement, "the REC Male Corpse must contain dead power cell 808");
    const [cellX, cellY, cellWidth, cellHeight] = cellElement.screen_rect;
    await game.input.set("pointer.position", [
      cellX + cellWidth / 2,
      cellY + cellHeight / 2,
    ]);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 2 });
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === deadCell.id,
      ),
      "clicking the corpse loot entry must take the dead power cell",
    );
    await game.entities.sendMessage(station.id, { type: "Frob" });
    await game.step({ frames: 2 });
    assert.equal(
      (await game.player.inventory()).items.filter(
        (item) => item.name === "Power Cell",
      ).length,
      1,
      "the production breaker chain requires rec1's recharged Power Cell",
    );

    // Stage the same fixed camera in the dark basketball-court corridor for
    // both captures. This framing looks toward the authored light at z=-213.
    await game.player.teleport({ x: -5.3, y: -0.5, z: -213.0 });
    await game.input.set("head.look", [180, 0]);
    await game.step({ frames: 2 });

    await game.entities.sendMessage(button.id, { type: "TurnOff" });
    await game.step({ frames: 2 });
    const off = await game.screenshot("rec-dynamic-lights-off.png");
    assert.equal(
      property(await game.entities.detail(omni.id), "AnimLightIntensity"),
      "0.000",
      "TurnOff must remove the authored lightmap contribution",
    );
    assert.equal((await game.save("rec-dynamic-lights-off")).success, true);
    const beforeOnSequence =
      (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;

    // Frob the real Aux Power receptor. ObjConsumeButton consumes the carried
    // cell and sends TurnOn to Light_button 77, which fans out to the authored
    // animated lights. This is the retail 807 -> 77 -> light chain.
    await game.entities.sendMessage(breaker.id, { type: "Frob" });
    await game.step({ frames: 2 });
    const on = await game.screenshot("rec-dynamic-lights-on.png");

    const recentMessages = (await game.messages.recent()).messages;
    const breakerMessages = recentMessages.filter(
      (message) =>
        message.sequence > beforeOnSequence &&
        message.payload === "TurnOn" &&
        message.from?.template_id === AUX_POWER_BREAKER_OBJECT &&
        message.to.template_id === LIGHT_BUTTON_OBJECT,
    );
    assert.equal(
      breakerMessages.length,
      1,
      "the production Aux Power receptor must activate Light_button 77",
    );
    const onMessages = recentMessages.filter(
      (message) =>
        message.sequence > beforeOnSequence &&
        message.payload === "TurnOn" &&
        message.from?.template_id === LIGHT_BUTTON_OBJECT &&
        message.to.template_id !== LIGHT_BUTTON_OBJECT,
    );
    assert.equal(
      onMessages.length,
      17,
      "the production Simple Button must still reach every authored light target",
    );
    assert.equal(
      (await game.player.inventory()).items.filter(
        (item) => item.name === "Power Cell",
      ).length,
      0,
      "the production Aux Power receptor must consume the charged cell",
    );

    assert.notEqual(
      sha256(on.full_path),
      sha256(off.full_path),
      "restoring auxiliary power must visibly change the fixed corridor render",
    );
    assert.equal(
      property(await game.entities.detail(omni.id), "AnimLightIntensity"),
      "1.000",
      "BaseLight must retain the live full-brightness state on the authored property",
    );

    // `P$AnimLight` is a registered Dark property, so the non-default off
    // state must restore through the normal mission save path. Entity ids are
    // rediscovered because loading creates a new ECS world.
    assert.equal((await game.load("rec-dynamic-lights-off")).success, true);
    await game.step({ frames: 2 });
    const [restoredOmni] = await game.entities.byTemplate(
      ANIM_OMNI_LIGHT_OBJECT,
    );
    assert.ok(restoredOmni);
    assert.equal(
      property(
        await game.entities.detail(restoredOmni.id),
        "AnimLightIntensity",
      ),
      "0.000",
      "the switched-off light state must survive save/load",
    );
    const restoredOff = await game.screenshot(
      "rec-dynamic-lights-restored-off.png",
    );
    assert.notEqual(
      sha256(restoredOff.full_path),
      sha256(on.full_path),
      "loading the off state must remove the visible light contribution again",
    );
  },
);
