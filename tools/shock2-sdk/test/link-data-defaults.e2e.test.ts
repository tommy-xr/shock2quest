import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Sparse LD$ flavors use typed defaults. Missing physical offsets derive the
// authored child-parent displacement; an explicit zero still means the parent
// origin. earth.mis carries six PhysAttach links and no data records, so both
// resting placement and movement must preserve the collision shell's pose.
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

test(
  "earth tram panels follow their parent while preserving authored offsets",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 30 });
    const assertFollow = async () => {
      const { entities } = await game.entities.list({ filter: "Large Tram" });
      const car = entities.find((e) => e.name === "Large Tramcar");
      const panels = entities.filter((e) => e.name !== "Large Tramcar");
      assert.ok(car);
      assert.equal(panels.length, 6);
      await game.entities.sendMessage(car.id, { type: "TurnOn" });
      await game.step({ frames: 120 });
      const movedCar = await game.entities.detail(car.id);
      const travel = movedCar.position.map((v, i) => v - car.position[i]);
      // The authored earth tram path is only 0.3 world units long.
      assert.ok(Math.hypot(...travel) > 0.2, `tram must move: ${travel}`);
      for (const panel of panels) {
        const moved = await game.entities.detail(panel.id);
        const relativeError = moved.position.map(
          (v, i) => v - panel.position[i] - travel[i],
        );
        assert.ok(
          Math.hypot(...relativeError) < 0.02,
          `${panel.name} must follow tram without snapping: error=${relativeError}`,
        );
      }
    };
    const save = `sparse_attachments_${Date.now()}`;
    assert.equal((await game.save(save)).success, true);
    await assertFollow();
    assert.equal((await game.load(save)).success, true);
    await assertFollow();
  },
);

test(
  "shodan sparse TPath retains the authored 1234 to 1307 edge",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "shodan.mis" });
    const [source] = await game.entities.byTemplate(1234);
    const [target] = await game.entities.byTemplate(1307);
    assert.ok(source);
    assert.ok(target);
    const detail = await game.entities.detail(source.id);
    assert.ok(
      detail.outgoing_links.some((link) =>
        link.link_type.startsWith("TPath") && link.target_id === target.id),
      `sparse path must remain connected: ${JSON.stringify(detail.outgoing_links)}`,
    );
  },
);
