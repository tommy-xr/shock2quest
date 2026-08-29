import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// VR recharge-station regression coverage: holding an item to a Recharging
// Station must charge it ONCE per interaction, not every frame.
//
// The station receives a Hover message every frame the hand ray touches it;
// before the contact latch in energy_station.rs, each of those replayed the
// charge + "recharge" activate sound (60/s of sound spam). Negative-first:
// reverting the latch makes the steady-state assertion below fail with ~180
// extra recharge sounds in the 3-second hover window.
//
// The setup drives the real VR interaction: physically grab a world Dead
// Power Cell with the simulated right hand, then aim the held item at the
// station and hold it there.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

type Vec3 = [number, number, number];

const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const norm = (v: Vec3): Vec3 => {
  const len = Math.hypot(...v);
  return [v[0] / len, v[1] / len, v[2] / len];
};

/** Quaternion [x,y,z,w] rotating the hand ray axis (0,0,-1) onto `dir`. */
function aimQuat(dir: Vec3): [number, number, number, number] {
  const a: Vec3 = [0, 0, -1];
  const d = norm(dir);
  // q = (w: 1 + a.d, xyz: a x d), normalized.
  const w = 1 + (a[0] * d[0] + a[1] * d[1] + a[2] * d[2]);
  const x = a[1] * d[2] - a[2] * d[1];
  const y = a[2] * d[0] - a[0] * d[2];
  const z = a[0] * d[1] - a[1] * d[0];
  const len = Math.hypot(x, y, z, w);
  return [x / len, y / len, z / len, w / len];
}

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

    const settled = (await game.entities.detail(cell.id)).position as Vec3;
    const p0 = await game.player.position();
    const playerPos: Vec3 = [p0.x, p0.y, p0.z];

    // Hand channels are pawn-local; put the hand half a unit above the cell
    // pointing straight down, then squeeze to grab.
    const local = sub(settled, playerPos);
    await game.input.set("right_hand.position", [local[0], local[1] + 0.5, local[2]]);
    await game.input.set("right_hand.rotation", [-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });

    const held = (await game.player.inventory()).items.find(
      (i) => i.location === "right_hand",
    );
    assert.ok(held, "the Dead Power Cell should be grabbed into the right hand");
    assert.equal(held.name, "Dead Power Cell");

    // --- Hold the item to a Recharging Station. ---
    // Pin one specific station by mission object id (stable across runs):
    // obj 506 faces pawn +X, matching the stand-and-aim geometry below.
    const stations = (
      await game.entities.list({ filter: "Recharging Station", limit: 10 })
    ).entities;
    const station = stations.find((s) => s.template_id === 506);
    assert.ok(station, "medsci1 should contain Recharging Station obj 506");

    // Stand pawn-forward (-X) of the station; aim the held hand at it.
    await game.player.teleport({
      x: station.position[0] + 3,
      y: station.position[1],
      z: station.position[2],
    });
    const p1 = await game.player.position();
    const pawn: Vec3 = [p1.x, p1.y, p1.z];
    const handLocal: Vec3 = [-0.55, 1.0, -0.2];
    const handWorld: Vec3 = [
      pawn[0] + handLocal[0],
      pawn[1] + handLocal[1],
      pawn[2] + handLocal[2],
    ];
    const target: Vec3 = [
      station.position[0],
      station.position[1],
      station.position[2],
    ];
    await game.input.set("right_hand.position", handLocal);
    await game.input.set("right_hand.rotation", aimQuat(sub(target, handWorld)));

    // The audio log is a bounded ring buffer, so window by sequence snapshots.
    const lastSequence = async () =>
      Math.max(0, ...(await game.audio.recent()).sounds.map((s) => s.sequence));
    const rechargesSince = async (sequence: number) =>
      (await game.audio.recent()).sounds.filter(
        (s) => s.sequence > sequence && s.sample === "recharge",
      ).length;

    // Contact second: the charge event fires. The charge replaces the dead
    // cell with a charged one (a new entity), which is itself charged once as
    // a fresh contact - so allow up to 2 events, but nothing per-frame.
    const beforeContact = await lastSequence();
    await game.step({ frames: 60 });
    const duringContact = await rechargesSince(beforeContact);
    assert.ok(
      duringContact >= 1 && duringContact <= 2,
      `expected 1-2 charge events at contact, saw ${duringContact}`,
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
