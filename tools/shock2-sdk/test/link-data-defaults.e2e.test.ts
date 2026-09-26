import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// LD$ link-data chunks are sparse, and a link with no record of its own only
// takes a zero-filled default where its reader parses zeroes as "unspecified"
// (`LinkDefinitionWithData::defaults_missing_records`).
//
// earth.mis is the counter-example that makes that opt-in necessary: its
// LD$PhysAtta chunk declares a 12-byte record and holds NONE, for six
// L$PhysAttach links. A zeroed PhysAttach offset does not mean "no offset
// authored" - it means "sit exactly on the parent's origin", so defaulting
// those records welds the intro tram's whole collision shell (roof, sides,
// front, back) onto the tramcar's origin within a couple of seconds.
//
// Opt-in (this file only):
//   tsc && SHOCK2_E2E=1 node --test dist/test/link-data-defaults.e2e.test.js
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "earth tram panels keep their authored placement, not the tramcar's origin",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });

    const panelsAt = async () => {
      const { entities } = await game.entities.list({ filter: "Large Tram" });
      const panels = entities.filter((e) => e.name !== "Large Tramcar");
      const car = entities.find((e) => e.name === "Large Tramcar");
      assert.ok(car, "expected a Large Tramcar in earth.mis");
      assert.equal(panels.length, 6, `expected 6 tram panels, got ${panels.length}`);
      return { panels, car };
    };

    const { panels: before, car } = await panelsAt();

    // The panels are authored away from the car's own origin; if they were
    // already coincident this test could not tell the two apart.
    const authored = before[0].position;
    assert.ok(
      Math.hypot(...authored.map((v, i) => v - car.position[i])) > 0.1,
      "panels and tramcar start coincident - test cannot distinguish the regression",
    );

    await game.step({ frames: 120 });

    const { panels: after } = await panelsAt();
    for (const panel of after) {
      const start = before.find((p) => p.id === panel.id);
      assert.ok(start, `panel ${panel.id} disappeared`);
      assert.deepEqual(
        panel.position,
        start.position,
        `${panel.name} moved off its authored placement to the tramcar origin`,
      );
    }
  },
);
