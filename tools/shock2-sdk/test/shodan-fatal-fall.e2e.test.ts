import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression for #724. The authored Teleport5Tripwire is SHODAN's
// finale entrance and sends the player to Seat1 on the north walkway. From
// there, ordinary forward locomotion can leave the walkway and drop through
// the center void. Before the fix, the player remained alive forever at
// y~-121 with no way to recover.
//
// The mission objects are discovered through their stable authored template
// ids. Runtime entity ids change on every launch and are deliberately unused.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const TELEPORT_5_TRIPWIRE = 1354;
const SHODAN_ENTRANCE_SEAT = 776;

test(
  "shodan.mis: walking off the finale walkway does not leave the player alive in the void",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8480),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
      rustLog: process.env.SHOCK2_RUST_LOG,
    });
    await game.step({ frames: 5 });

    const tripwires = await game.entities.byTemplate(TELEPORT_5_TRIPWIRE);
    assert.equal(
      tripwires.length,
      1,
      `expected authored Teleport5Tripwire ${TELEPORT_5_TRIPWIRE}`,
    );
    const seats = await game.entities.byTemplate(SHODAN_ENTRANCE_SEAT);
    assert.equal(
      seats.length,
      1,
      `expected authored SHODAN entrance Seat1 ${SHODAN_ENTRANCE_SEAT}`,
    );
    const [tripwire] = tripwires;
    const [seat] = seats;

    // Enter the arena through its real authored Tripwire -> SwitchLink ->
    // Teleport-to-Seat path. Teleport only stages inside the remote trigger;
    // the mission scripts perform the arena transfer.
    const [tripX, tripY, tripZ] = tripwire.position;
    await game.player.teleport({ x: tripX, y: tripY, z: tripZ });
    await game.step({ frames: 10 });

    const arrival = await game.player.position();
    const [seatX, seatY, seatZ] = seat.position;
    assert.ok(
      Math.hypot(arrival.x - seatX, arrival.z - seatZ) < 2 &&
        Math.abs(arrival.y - seatY) < 3,
      `Teleport5Tripwire should enter the real arena at Seat1; ` +
        `seat=${JSON.stringify(seat.position)} arrival=${JSON.stringify(arrival)}`,
    );

    const hpBefore = (await game.info()).player.hit_points;
    assert.ok(
      hpBefore !== null && hpBefore > 0,
      `the player must enter the arena alive, got HP ${hpBefore}`,
    );

    // Seat1 faces the north walkway. Aim down its +Z centerline and use the
    // production thumbstick path to cross the edge into the central void.
    await game.input.lookAtWorldPoint([seatX, arrival.y + 1.6, seatZ + 20]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 180 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const fallen = await game.player.position();
    const hpAfter = (await game.info()).player.hit_points;
    assert.ok(
      fallen.y < arrival.y - 20,
      `ordinary locomotion must genuinely leave the walkway and fall; ` +
        `arrival=${JSON.stringify(arrival)} fallen=${JSON.stringify(fallen)}`,
    );
    assert.ok(
      hpAfter !== null && hpAfter <= 0,
      `a fatal unsupported fall must not leave the player alive in the void; ` +
        `HP ${hpBefore} -> ${hpAfter}, position=${JSON.stringify(fallen)}`,
    );
  },
);
