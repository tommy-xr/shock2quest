import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";

// End-to-end coverage for command1's authored moving-terrain assembly. Runtime
// ids are intentionally discovered every launch: mission object ids are the
// stable handles for the attached button/front wall, while the root has a
// unique authored name.
//
// Negative-first: before #663, only the Tram root/floor advanced. Its
// PhysAttach children remained at the station, so Tram Front pinned a
// correctly boarded passenger at x=-374.16 while the root moved twelve units.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const TRAM_BUTTON_OBJECT = 199;
const TRAM_FRONT_OBJECT = 152;

const delta = (after: Vec3, before: Vec3): Vec3 => [
  after[0] - before[0],
  after[1] - before[1],
  after[2] - before[2],
];

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

test(
  "command1.mis: authored tram assembly carries a boarded passenger",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8184),
    });
    await game.step({ frames: 5 });

    const tram = only(
      (await game.entities.list({ filter: "Tram", limit: 30 })).entities.filter(
        (entity) => entity.name === "Tram",
      ),
      "authored Tram root",
    );
    const tramFront = only(
      await game.entities.byTemplate(TRAM_FRONT_OBJECT),
      "Tram Front mission object",
    );
    const button = only(
      await game.entities.byTemplate(TRAM_BUTTON_OBJECT),
      "in-car tram button mission object",
    );

    // Teleport is setup only. Board through the open side doorway with the
    // bounded, shape-cast-validated move endpoint before asserting carry.
    await game.player.teleport({ x: -377.6, y: -16.4, z: 5.2 });
    await game.step({ frames: 2 });
    const boarding = await game.player.moveTo({
      x: tram.position[0] - 0.25,
      y: -16.4,
      z: tram.position[2] + 0.08,
    });
    assert.equal(boarding.moved, true, "player should walk through the tram doorway");
    assert.equal(boarding.blocked, false, "open tram doorway should not block boarding");
    await game.step({ frames: 10 });

    const playerBefore = await game.player.position();
    const tramBefore = (await game.entities.detail(tram.id)).position;
    const frontBefore = (await game.entities.detail(tramFront.id)).position;
    const buttonBefore = (await game.entities.detail(button.id)).position;
    assert.ok(
      Math.abs(playerBefore.x - tramBefore[0]) < 1 &&
        Math.abs(playerBefore.z - tramBefore[2]) < 1,
      `player must be inside the car before dispatch: player=${JSON.stringify(playerBefore)}, tram=${JSON.stringify(tramBefore)}`,
    );

    const aim = await game.player.aimAt(button, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(aim.entity_id, button.id, JSON.stringify(aim));
    assert.equal(aim.visibility.state, "visible", JSON.stringify(aim));
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 60 });

    const playerAfter = await game.player.position();
    const tramAfter = (await game.entities.detail(tram.id)).position;
    const frontAfter = (await game.entities.detail(tramFront.id)).position;
    const buttonAfter = (await game.entities.detail(button.id)).position;
    const tramTravel = delta(tramAfter, tramBefore);
    const frontTravel = delta(frontAfter, frontBefore);
    const buttonTravel = delta(buttonAfter, buttonBefore);
    const playerTravel = playerAfter.x - playerBefore.x;

    assert.ok(
      tramTravel[0] > 10,
      `production button should dispatch the tram, travel=${JSON.stringify(tramTravel)}`,
    );
    assert.ok(
      Math.abs(frontTravel[0] - tramTravel[0]) < 0.05,
      `PhysAttach front wall must track the root: front=${JSON.stringify(frontTravel)}, tram=${JSON.stringify(tramTravel)}`,
    );
    assert.ok(
      Math.abs(buttonTravel[0] - tramTravel[0]) < 0.05,
      `PhysAttach button must track the root: button=${JSON.stringify(buttonTravel)}, tram=${JSON.stringify(tramTravel)}`,
    );
    assert.ok(
      playerTravel > 10 && Math.abs(playerAfter.x - tramAfter[0]) < 2,
      `stationary passenger must remain aboard: player travel=${playerTravel}, tram travel=${tramTravel[0]}, relative x=${playerAfter.x - tramAfter[0]}`,
    );
  },
);
