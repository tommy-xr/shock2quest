import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { norm } from "./helpers/frontend-menu.js";

// The developer cheat pad (`shock2vr::cheat_pad`) is a Game-owned overlay
// gated on the `cheats` dev param. Two buttons rain items around the player.
//
// Negative-first: with the gate off, the open action and every click that
// follows it must do nothing at all - which is the first half of this test.
// Verified by removing both gate checks: the test then fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The pad is a `list_scroll` list on the Developer page's authored widget
// rects (GAMELODR.BIN: list pane 261,54 202x290 and Done 527,405 95x62), at
// the same 19px row pitch the debug-scene launcher uses - see
// `cheat_pad::row_rect`. The two shipped cheats fit one page, so there is no
// scroll gutter and rows span the full pane width.
const row = (index: number): [number, number] => [261 + 202 / 2, 54 + index * 19 + 19 / 2];
const RAIN_WEAPONS = row(0);
const RAIN_MODULES = row(1);
const DONE: [number, number] = [527 + 95 / 2, 405 + 62 / 2];
/** SIMR.BIN pause entries: five 179x76 buttons at x=400, top 20, 92px pitch. */
const PAUSE_CONTINUE: [number, number] = [400 + 179 / 2, 20 + 76 / 2];

/** Click a canvas point with the flat pointer, as a real rising edge. */
async function click(game: GameServer, [x, y]: [number, number]): Promise<void> {
  await game.input.set("pointer.position", norm(x, y));
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
}

async function ids(game: GameServer, filter: string): Promise<Set<number>> {
  const { entities } = await game.entities.list({ filter, limit: 200 });
  return new Set(entities.map((e) => e.id));
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

    const wrenchesBefore = await ids(game, "Wrench");
    const modulesBefore = (await ids(game, "EXP")).size;
    const nanitesBefore = (await ids(game, "Nanites")).size;

    // NEGATIVE: gate off. The action is accepted by HTTP but must be inert,
    // so a click where the button would be changes nothing in the world.
    await game.input.trigger("ToggleCheatPad");
    await game.step({ frames: 5 });
    await click(game, RAIN_WEAPONS);
    assert.equal(
      (await ids(game, "Wrench")).size,
      wrenchesBefore.size,
      "an ungated cheat pad must not rain anything",
    );

    // Gate on: now the pad opens and its buttons work.
    await game.devParams.set("cheats", 1);
    await game.input.trigger("ToggleCheatPad");
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).paused,
      true,
      "an open cheat pad suspends the scene, so automation must see it as paused",
    );

    await click(game, RAIN_WEAPONS);
    const rained = [...(await ids(game, "Wrench"))].filter((id) => !wrenchesBefore.has(id));
    assert.equal(rained.length, 1, `rain weapons should have added exactly one wrench`);

    // The rained weapon is a real dynamic pickup: it owns a physics body, and
    // once the pad closes and the world runs again it settles.
    const { bodies } = await game.physics.bodies({ entityId: rained[0] });
    assert.ok(bodies.length > 0, "a rained item should own a physics body");
    assert.equal(bodies[0].body_type, "dynamic");

    await click(game, DONE);
    await game.step({ frames: 180 });
    const settled = (await game.physics.bodies({ entityId: rained[0] })).bodies[0];
    const speed = Math.hypot(...settled.velocity);
    assert.ok(speed < 0.5, `the rained wrench should have settled, moving at ${speed}`);

    // Reopening reaches the second button, which rains a different spread.
    await game.input.trigger("ToggleCheatPad");
    await game.step({ frames: 5 });
    await click(game, RAIN_MODULES);
    assert.equal(
      (await ids(game, "EXP")).size,
      modulesBefore + 4,
      "rain modules should have added four cyber-module stacks",
    );
    assert.equal(
      (await ids(game, "Nanites")).size,
      nanitesBefore + 4,
      "rain modules should have added four nanite piles",
    );

    // At most one Game overlay is up at a time: opening the pause menu over
    // the pad must take the pad down, or the pad's opaque backdrop would hide
    // the menu that is actually consuming the clicks (and "Quit to Main Menu"
    // sits under the pad's own rows).
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 5 });
    const afterPause = (await ids(game, "EXP")).size;
    await click(game, RAIN_WEAPONS);
    assert.equal(
      (await ids(game, "EXP")).size,
      afterPause,
      "the cheat pad must be gone once the pause menu takes over",
    );
    // ...and the pause menu really is the one on screen: its Continue button
    // resumes the world.
    await click(game, PAUSE_CONTINUE);
    assert.equal((await game.info()).paused, false, "Continue should resume the mission");
  },
);
