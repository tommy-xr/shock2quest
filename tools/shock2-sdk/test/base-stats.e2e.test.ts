import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the start marker's `P$BaseStats`.
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: `P$BaseStats` was not parsed, so Earth's new player had PSI 1
// (and the psi pool that goes with it) instead of the marker's PSI 5.
//
// Dark clones a mission's PlayerFactory start marker onto the player only
// when it creates one: a new game or a direct launch. A deck transition
// carries the player's own stats across.
//   earth.mis Starting_Location 243: BaseStats 1/1/5/1/1 (STR END PSI AGI CYB)
//   hydro1.mis START_LOCATION 68:    BaseStats 6/6/6/6/6 (a DromEd test loadout)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "earth: a new player takes the start marker's PSI 5, without changing the career sheet",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 10 });
    const before = (await game.info()).player;
    assert.equal(before.effective_stats?.psionic_ability, 5, "the marker floors PSI at 5");
    assert.equal(before.stats?.psionic_ability, 1, "the career sheet keeps its trained PSI");
    assert.equal(before.effective_stats?.strength, 1);

    const saveName = `base_stats_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    const reloaded = (await game.info()).player;
    assert.equal(reloaded.effective_stats?.psionic_ability, 5, "an Earth save keeps it");
    assert.equal(reloaded.max_psi_points, before.max_psi_points);
  },
);

test(
  "a deck transition keeps the player's stats instead of the destination's marker",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 5 });
    const medsci = (await game.info()).player;
    await game.transitionLevel("hydro1.mis");
    await game.step({ frames: 5 });
    const hydro = (await game.info()).player;
    assert.deepEqual(
      [hydro.effective_stats?.strength, hydro.effective_stats?.psionic_ability],
      [medsci.effective_stats?.strength, medsci.effective_stats?.psionic_ability],
      "hydro1's all-6 marker must not apply mid-campaign",
    );
    assert.equal(hydro.max_psi_points, medsci.max_psi_points);
  },
);

test(
  "horde keeps its provisioned PSI 2 instead of Earth's tutorial marker",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth_horde" });
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.effective_stats?.psionic_ability, 2);
  },
);

test(
  "rec1 direct launch: a Strength purchase starts from the marker's STR 3",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "rec1.mis" });
    await game.step({ frames: 10 });
    const before = (await game.info()).player;
    assert.equal(before.effective_stats?.strength, 3, "rec1's marker floors STR at 3");
    await game.player.setStats({ cyber_modules: 50 });

    const trainer = (await game.entities.list({ filter: "Stats Trainer" })).entities[0];
    assert.ok(trainer, "rec1 should contain a Stats Trainer");
    const [tx, ty, tz] = (await game.entities.detail(trainer.id)).position;
    let opened = null;
    for (const [dx, dz] of [[0, 1.2], [0, -1.2], [1.2, 0], [-1.2, 0]]) {
      await game.player.teleport({ x: tx + dx, y: ty + 0.5, z: tz + dz });
      await game.step({ frames: 30 });
      await game.entities.sendMessage(trainer.id, { type: "Frob" });
      await game.step({ frames: 5 });
      opened = (await game.ui.state()).active_panel;
      if (opened) break;
    }
    assert.ok(opened, "the Stats Trainer panel should open");
    const strength = opened.elements.find(e => e.kind === "button" && e.label === "Strength");
    assert.ok(strength);
    const [x, y, w, h] = strength.screen_rect;
    await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });

    const after = (await game.info()).player;
    assert.equal(after.effective_stats?.strength, 4, "the purchase raises STR 3 -> 4");
    assert.equal(after.stats?.strength, 4, "the sheet takes the purchased level");
    assert.ok((after.stats?.cyber_modules ?? 50) < 50, "modules were spent");
  },
);
