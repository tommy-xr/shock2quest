import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The developer cheat pad (`shock2vr::cheat_pad`) is a Game-owned overlay
// gated on the `cheats` dev param. Two buttons rain items around the player.
//
// Negative-first: with the gate off, the open action and every click that
// follows it must do nothing at all - which is the first half of this test,
// and which fails against a build that opens the pad unconditionally.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CANVAS_W = 640;
const CANVAS_H = 480;
// The pad rides the Developer page's authored widget rects (GAMELODR.BIN,
// fallback list pane 261,54,202,290 and Done 527,405,95,62), with 40px rows
// inset 8px on a 56px pitch - see `cheat_pad::button_rect`.
const row = (index: number): [number, number] => [
  (261 + 202 / 2) / CANVAS_W,
  (54 + 8 + index * 56 + 40 / 2) / CANVAS_H,
];
const RAIN_WEAPONS = row(0);
const RAIN_MODULES = row(1);
const DONE: [number, number] = [(527 + 95 / 2) / CANVAS_W, (405 + 62 / 2) / CANVAS_H];

/** Click a canvas point with the flat pointer, as a real rising edge. */
async function click(game: GameServer, [u, v]: [number, number]): Promise<void> {
  await game.input.set("pointer.position", [u, v]);
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
}

async function count(game: GameServer, filter: string): Promise<number> {
  return (await game.entities.list({ filter, limit: 200 })).entities.length;
}

test(
  "the cheat pad is unreachable until the cheats option is on, then rains items",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 30 });

    const { params } = await game.devParams.list();
    const cheats = params.find((p) => p.key === "cheats");
    assert.ok(cheats, "the `cheats` gate must be registered");
    assert.equal(cheats.kind, "bool");
    assert.equal(cheats.value, 0, "cheats must be off by default");

    const wrenchesBefore = await count(game, "Wrench");
    const modulesBefore = await count(game, "EXP");
    const nanitesBefore = await count(game, "Nanites");

    // NEGATIVE: gate off. The action is accepted by HTTP but must be inert,
    // so a click where the button would be changes nothing in the world.
    await game.input.trigger("ToggleCheatPad");
    await game.step({ frames: 5 });
    await click(game, RAIN_WEAPONS);
    assert.equal(
      await count(game, "Wrench"),
      wrenchesBefore,
      "an ungated cheat pad must not rain anything",
    );

    // Gate on: now the pad opens and its buttons work.
    await game.devParams.set("cheats", 1);
    await game.input.trigger("ToggleCheatPad");
    await game.step({ frames: 5 });

    await click(game, RAIN_WEAPONS);
    const wrenchesAfter = await count(game, "Wrench");
    assert.ok(
      wrenchesAfter > wrenchesBefore,
      `rain weapons should have added a wrench (${wrenchesBefore} -> ${wrenchesAfter})`,
    );

    // The rained weapon is a real dynamic pickup: it owns a physics body,
    // and once the pad closes and the world runs again it settles.
    const wrench = (await game.entities.list({ filter: "Wrench", limit: 200 })).entities
      .filter((e) => e.template_id === -928)
      .at(0);
    assert.ok(wrench, "the rained wrench should be the -928 template");
    const spawned = await game.physics.bodies({ entityId: wrench.id });
    assert.ok(spawned.bodies.length > 0, "a rained item should own a physics body");
    assert.equal(spawned.bodies[0].body_type, "dynamic");

    await click(game, DONE);
    await game.step({ frames: 180 });
    const settled = (await game.physics.bodies({ entityId: wrench.id })).bodies[0];
    const speed = Math.hypot(...settled.velocity);
    assert.ok(speed < 0.5, `the rained wrench should have settled, moving at ${speed}`);

    // Reopening reaches the second button, which rains a different spread.
    await game.input.trigger("ToggleCheatPad");
    await game.step({ frames: 5 });
    await click(game, RAIN_MODULES);
    assert.equal(
      await count(game, "EXP"),
      modulesBefore + 4,
      "rain modules should have added four cyber-module stacks",
    );
    assert.equal(
      await count(game, "Nanites"),
      nanitesBefore + 4,
      "rain modules should have added four nanite piles",
    );
  },
);
