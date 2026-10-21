import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, otherHand, vrClimbLadder, vrGrab, vrPull, vrReleaseOnto } from "../src/index.js";
import type { Vec3 } from "../src/index.js";

// Real mission ladder tops (#2376): climb the ladder, take the floor at its
// top (a grace hold off the ladder), and the exit must work - an assisted
// landing a release lands on, or, where the held floor offers none, the
// pull-up vault as a fallback. Tops from a sweep of the shipped missions.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

interface Top {
  mission: string;
  /** The ladder column's top (rail top, centre). */
  top: Vec3;
  /** Where the free hand took the floor in the sweep (searched around). */
  hold: Vec3;
}

const TOPS: Top[] = [
  { mission: "medsci1.mis", top: [-17.54, -1.59, 14.45], hold: [-17.54, -1.6, 14.25] },
  { mission: "eng2.mis", top: [4.85, -12.8, -278.35], hold: [4.65, -12.8, -278.35] },
  { mission: "rec1.mis", top: [22.24, -4.96, -232.81], hold: [22.44, -5.2, -232.81] },
  { mission: "rick1.mis", top: [94.48, 55.6, -6.38], hold: [94.28, 56.0, -6.68] },
  // Its held floor offers no landing: the vault takes over.
  { mission: "rick1.mis", top: [94.48, 55.6, -14.38], hold: [94.28, 56.0, -14.68] },
];

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const horizontal = (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[2] - b[2]);

for (const { mission, top, hold } of TOPS) {
  test(
    `${mission} (VR): the ladder top at ${top.join(",")} exits onto its floor`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({ mission, debugFlags: ["--vr"] });
      await game.devParams.set("cheat", 1);
      await game.step({ frames: 10 });
      // The ladder entities on that column, bottom to top.
      const { entities } = await game.entities.list({ filter: "*adder*", limit: 1000 });
      const column = [];
      for (const entity of entities) {
        if (horizontal(entity.position, top) > 1.5) continue;
        try {
          const holds = await game.physics.ladder(entity.id);
          const rails = holds.rails.flat();
          if (rails.some((p) => horizontal(p, top) < 0.8)) column.push(holds);
        } catch {
          // Not a ladder with holds.
        }
      }
      assert.ok(column.length > 0, "the ladder is there");
      const normal = column[0].normal;
      const bottom = Math.min(...column.flatMap((h) => h.rails.flat().map((p) => p[1])));
      // The climbing side faces away from the floor at the top.
      const away: Vec3 = [top[0] - hold[0], 0, top[2] - hold[2]];
      const side = dot([normal[0], 0, normal[2]], away) >= 0 ? 1 : -1;
      const n: Vec3 = [normal[0] * side, 0, normal[2] * side];
      await game.player.teleport({ x: top[0] + n[0] * 0.6, y: bottom + 1.4, z: top[2] + n[2] * 0.6 });
      await game.step({ frames: 30 });
      const near: Vec3 = [top[0] + n[0] * 0.05, bottom + 1.2, top[2] + n[2] * 0.05];
      const { anchor } = await vrClimbLadder(game, { near, untilY: top[1] - 0.7 });

      // The floor past the top, near where the sweep took it, in reach.
      const free = otherHand(anchor);
      let held = null;
      search: for (const dy of [0.05, 0.2, -0.1, 0.35]) {
        for (const d of [0, 0.1, -0.1, 0.2]) {
          const at: Vec3 = [hold[0] - n[0] * d, hold[1] + dy, hold[2] - n[2] * d];
          if ((await game.physics.grip(at)).grip?.kind !== "ledge") continue;
          try {
            held = await vrGrab(game, free, at);
            break search;
          } catch {
            // Out of reach.
          }
        }
      }
      assert.ok(held, "the floor at the top is in reach");
      assert.equal(held.kind, "surface", "the floor off a ladder is a grace hold");
      await game.input.set(`${free}_hand.world_target`, null);
      await game.step({ frames: 2 });
      const shown = (await game.info()).player.climb.release;
      if (shown.mode === "assisted") {
        const { landed } = await vrReleaseOnto(game);
        assert.ok(landed[1] > top[1] - 0.5, `landed on the floor at the top: ${landed}`);
      } else {
        await vrPull(game, free, [0, -1.2, 0], 40, (p) => p.climb.vaulting);
        assert.equal((await game.info()).player.climb.vaulting, true, `neither an assisted landing nor a vault: ${JSON.stringify(shown)}`);
      }
    },
  );
}
