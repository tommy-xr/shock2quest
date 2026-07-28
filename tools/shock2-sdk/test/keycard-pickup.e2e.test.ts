import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable mission-object ids. Runtime ids are rediscovered after every launch
// and level transition.
const REC2_CREW_CORPSE = 419;
const REC2_CREW_KEY = 996;
const REC1_CREW_SLOT = 293;
const REC1_CREW_DOORS = [1580, 1582] as const;

test(
  "Rec Crew Key loot grants access, remains visible, and opens the Crew gate",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8193),
    });
    await game.step({ frames: 5 });

    const [corpse] = await game.entities.byTemplate(REC2_CREW_CORPSE);
    const [keycard] = await game.entities.byTemplate(REC2_CREW_KEY);
    assert.ok(corpse, "expected rec2 female corpse mission object 419");
    assert.equal(keycard?.name, "Rec Crew Key", "expected Rec Crew Key object 996");

    // Production loot flow: open the corpse's ContainerGui and click the
    // contained card. MOVE | SCRIPT must dispatch Frob rather than bypassing
    // the card's FrobQB and internal keycard scripts.
    await teleportVerified(game, {
      x: corpse.position[0] + 1.0,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1.0,
    });
    await game.entities.sendMessage(corpse.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel, "frobbing corpse 419 should open its loot panel");
    const cardButton = panel.elements.find(
      (element) =>
        element.kind === "button" &&
        element.entity_id === keycard.id &&
        element.label === "Rec Crew Key",
    );
    assert.ok(cardButton, "corpse 419 should expose the Rec Crew Key loot button");
    await clickUiElement(game, cardButton);

    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((item) => item.entity_id === keycard.id)?.location,
      "inventory",
      `the physical card must remain in the backpack; got ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      await game.quests.get("crewcahd"),
      "incomplete",
      "the card's authored FrobQB quest bit must run during loot Take",
    );

    // Key access is persistent player state rather than inferred from the
    // physical inventory item. Carry it through the real mission-load path,
    // then exercise the matching region-32/lock-0 slot and authored doors.
    await game.transitionLevel("rec1.mis");
    await game.step({ frames: 5 });

    const [slot] = await game.entities.byTemplate(REC1_CREW_SLOT);
    assert.ok(slot, "expected rec1 Crew card slot mission object 293");
    const doors = await Promise.all(
      REC1_CREW_DOORS.map(async (objectId) => {
        const [door] = await game.entities.byTemplate(objectId);
        assert.ok(door, `expected rec1 Crew door mission object ${objectId}`);
        return door;
      }),
    );
    const before = doors.map((door) => door.position);

    await game.entities.sendMessage(slot.id, { type: "Frob" });
    await game.step({ frames: 180 });

    for (const [index, door] of doors.entries()) {
      const after = (await game.entities.detail(door.id)).position;
      const displacement = Math.hypot(
        after[0] - before[index][0],
        after[1] - before[index][1],
        after[2] - before[index][2],
      );
      assert.ok(
        displacement > 1.0,
        `Crew slot 293 must open door ${REC1_CREW_DOORS[index]}; ` +
          `before=${before[index]}, after=${after}`,
      );
    }
  },
);
