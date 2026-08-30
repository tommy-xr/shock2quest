import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// VR recharge-station regression coverage: holding an item to a Recharging
// Station must charge it ONCE per interaction, not every frame.
//
// The station receives a Hover message every frame the hand ray touches it;
// before the contact latch in energy_station.rs, each of those replayed the
// charge + "recharge" activate sound (60/s of sound spam). Negative-first:
// reverting the latch makes the assertions below fail (~56 charge events in
// the contact second, and steady spam in the 3-second hover window).
//
// The setup drives the real VR interaction: physically grab a world Dead
// Power Cell with the simulated right hand, then aim the held item at the
// station and hold it there.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Mission object id (stable across runs, exposed as `template_id`) of one of
// medsci1's three Recharging Stations.
const STATION_OBJ = 506;

test(
  "medsci1.mis (VR): holding an item to the Recharging Station charges once, not per frame",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    // --- Grab a world Dead Power Cell with the right hand. ---
    const cells = (await game.entities.list({ filter: "Dead Power Cell", limit: 20 }))
      .entities;
    assert.ok(cells.length >= 1, "medsci1 should contain a Dead Power Cell");
    const cell = cells[0];

    // Stand near the cell and let it (and the player) settle.
    await game.player.teleport({
      x: cell.position[0],
      y: cell.position[1] + 1.6,
      z: cell.position[2] + 1.8,
    });
    await game.step({ frames: 30 });

    const settled = (await game.entities.detail(cell.id)).position;
    await aimVrHandAt(game, settled, 0.5);
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });

    const held = (await game.player.inventory()).items.find(
      (i) => i.location === "right_hand",
    );
    assert.ok(held, "the Dead Power Cell should be grabbed into the right hand");
    assert.equal(held.name, "Dead Power Cell");

    // --- Hold the item to the Recharging Station. ---
    const station = (await game.entities.byTemplate(STATION_OBJ)).find(
      (s) => s.name === "Recharging Station",
    );
    assert.ok(station, `medsci1 should contain Recharging Station obj ${STATION_OBJ}`);
    await game.player.teleport({
      x: station.position[0] + 3,
      y: station.position[1],
      z: station.position[2],
    });

    // The audio log is a bounded ring buffer, so window by sequence snapshots.
    const lastSequence = async () =>
      Math.max(0, ...(await game.audio.recent()).sounds.map((s) => s.sequence));
    const rechargesSince = async (sequence: number) =>
      (await game.audio.recent()).sounds.filter(
        (s) => s.sequence > sequence && s.sample === "recharge",
      ).length;

    // Snapshot BEFORE aiming: aimVrHandAt steps a few frames itself, and the
    // first of those already delivers the Hover that charges the item.
    const beforeContact = await lastSequence();
    // Keep the squeeze so the cell stays held while the hand re-aims.
    await aimVrHandAt(game, station.position, 1.5, /* squeeze */ 1);

    // Contact second: exactly one audible charge event. (The charge replaces
    // the dead cell with a charged one - a new entity - which is charged
    // quietly: the station suppresses the sound while a contact is fresh.)
    await game.step({ frames: 60 });
    const duringContact = await rechargesSince(beforeContact);
    assert.equal(
      duringContact,
      1,
      `expected exactly 1 audible charge event at contact, saw ${duringContact}`,
    );
    const heldAfter = (await game.player.inventory()).items.find(
      (i) => i.location === "right_hand",
    );
    assert.equal(heldAfter?.name, "Power Cell", "held dead cell should charge in place");

    // Steady state: three more seconds of continuous hover must charge nothing.
    const beforeSteady = await lastSequence();
    await game.step({ frames: 180 });
    const steadyState = await rechargesSince(beforeSteady);
    assert.equal(
      steadyState,
      0,
      `continuous hover replayed the recharge ${steadyState} more times`,
    );
  },
);
