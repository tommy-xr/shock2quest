import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end coverage for the FLAT-mode power-cell interaction (issue #518).
//
// Faithful flat behavior (unlike VR's hold-one-item-near-the-station):
//   1. Frobbing a Recharging Station recharges EVERY rechargeable item the
//      player carries at once - turning each Dead Power Cell in the backpack
//      into a charged Power Cell (in place, so it stays carried).
//   2. Frobbing the Aux Power receptor consumes a matching Power Cell from the
//      player's inventory and fires its switch-links (opening its Security Door).
//
// A frob is driven here via the Frob message injection, which dispatches the
// exact same MessagePayload::Frob the flat use-button emits at the reticle
// target (see the StdDoor door-frob test for the same convention; reticle
// targeting itself is covered by the campaign play-through).
//
// Negative-first: before #518 the EnergyStation script ignored Frob (it only
// handled the VR Hover/Collided messages), so frobbing the station left every
// Dead Power Cell dead. Reverting the energy_station.rs Frob arm makes the
// recharge assertion below fail (the dead cells stay "Dead Power Cell").
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Mission object ids (stable across runs; the debug runtime exposes them as
// `template_id`) - used to select specific instances without hardcoding the
// per-run runtime entity ids.
const SECURITY_DOOR_OBJ = 335; // switch-linked to the Aux Power receptor (obj 1128)
const RECEPTOR_OBJ = 1128; // Aux Power W/Out Battery, PropConsumeType "power cell"

test(
  "medsci1.mis: frobbing the Recharging Station recharges all carried cells, and the Aux Power receptor consumes one",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8161),
    });
    await game.step({ frames: 5 });

    // --- Stage: give the player both Dead Power Cells in the level. ---
    const deadCells = (
      await game.entities.list({ filter: "Dead Power Cell", limit: 20 })
    ).entities;
    assert.ok(
      deadCells.length >= 2,
      `expected >=2 Dead Power Cells in medsci1, found ${deadCells.length}`,
    );
    for (const cell of deadCells) {
      const res = await game.player.give(cell.id);
      assert.equal(res.success, true, `give dead cell ${cell.id}`);
    }

    const before = await game.player.inventory();
    const deadBefore = before.items.filter((i) => i.name === "Dead Power Cell");
    assert.equal(
      deadBefore.length,
      deadCells.length,
      "both dead cells should be carried before recharge",
    );

    // --- Recharge: frob the Recharging Station (recharge-all). ---
    const station = (
      await game.entities.list({ filter: "Recharging Station", limit: 10 })
    ).entities[0];
    assert.ok(station, "expected a Recharging Station in medsci1");

    await game.entities.sendMessage(station.id, { type: "Frob" });
    await game.step({ frames: 10 });

    const afterRecharge = await game.player.inventory();
    // Recharge-all: every Dead Power Cell became a charged Power Cell, and each
    // stays in the backpack (recharged in place, not orphaned in the world).
    assert.equal(
      afterRecharge.items.filter((i) => i.name === "Dead Power Cell").length,
      0,
      "no Dead Power Cell should remain after frobbing the station",
    );
    assert.equal(
      afterRecharge.items.filter((i) => i.name === "Power Cell").length,
      deadCells.length,
      "every dead cell should now be a charged Power Cell, still carried",
    );

    // --- Insert: frob the Aux Power receptor, consuming one Power Cell. ---
    const receptor = (
      await game.entities.byTemplate(RECEPTOR_OBJ)
    )[0];
    assert.ok(receptor, "expected the Aux Power receptor (obj 1128) in medsci1");

    const door = (await game.entities.byTemplate(SECURITY_DOOR_OBJ))[0];
    assert.ok(door, "expected the receptor's switch-linked Security Door");
    const doorYBefore = (await game.entities.detail(door.id)).position[1];

    const cellsBeforeInsert = afterRecharge.items.filter(
      (i) => i.name === "Power Cell",
    ).length;

    await game.entities.sendMessage(receptor.id, { type: "Frob" });
    await game.step({ frames: 60 });

    const afterInsert = await game.player.inventory();
    // One Power Cell was consumed (and left no dangling inventory link behind).
    assert.equal(
      afterInsert.items.filter((i) => i.name === "Power Cell").length,
      cellsBeforeInsert - 1,
      "frobbing the receptor should consume exactly one Power Cell",
    );

    // The receptor fired its switch-links: the Security Door slid open.
    const doorYAfter = (await game.entities.detail(door.id)).position[1];
    assert.ok(
      doorYAfter > doorYBefore + 1.0,
      `receptor should open its Security Door (y ${doorYBefore} -> ${doorYAfter})`,
    );
  },
);
