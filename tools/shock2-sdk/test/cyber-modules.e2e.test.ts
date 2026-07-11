import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";

// End-to-end test for the cyber-module currency (flat UI 6d / PR C1). Requires
// game assets in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Cyber modules are the game's upgrade currency. Before PR C1, Effect::AwardXP
// was a `warn!`-and-drop TODO and no balance existed anywhere. This test proves
// the closed loop: a persistent balance on the character sheet, incremented by
// the two real award sites (EXP traps via PropExp, EXP-cookie pickups via their
// stack count), surviving save/load and a level transition.
//
// Negative-first: on main, info().stats has no `cyber_modules` field (undefined,
// so the `=== 0` baseline assert fails) and TurnOn'ing the trap changes nothing.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8169);

/** Read the property `name`'s integer value from an entity's detail, or null. */
async function propInt(
  game: GameServer,
  id: number,
  name: string,
): Promise<number | null> {
  const detail = await game.entities.detail(id);
  const p = detail.properties.find((p) => p.name === name);
  return p ? Number(p.value) : null;
}

/** The player's current cyber-module balance from /v1/info. */
async function modules(game: GameServer): Promise<number | undefined> {
  return (await game.info()).player.stats?.cyber_modules;
}

test(
  "cyber modules: EXP traps + cookie pickups award a persistent balance",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `cyber_modules_e2e_${Date.now()}`;
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });
    await game.step({ frames: 5 });

    // Baseline: a fresh character has zero cyber modules. (On main this field
    // is absent -> undefined, so this assert fails: the negative test.)
    assert.equal(await modules(game), 0, "fresh character starts with 0 modules");

    // --- Award via an EXP trap (PropExp) ---
    // Discover an Experience Trap by name (runtime ids are not stable), pick one
    // that carries a positive PropExp, and read the exact award to assert on.
    const { entities } = await game.entities.list({ filter: "Experience Trap" });
    let trap: EntitySummary | undefined;
    let trapExp = 0;
    for (const e of entities) {
      const exp = await propInt(game, e.id, "Exp");
      if (exp && exp > 0) {
        trap = e;
        trapExp = exp;
        break;
      }
    }
    assert.ok(trap, "medsci1 should have an Experience Trap carrying PropExp");

    await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    await game.step({ frames: 3 });
    assert.equal(
      await modules(game),
      trapExp,
      `TurnOn'ing the EXP trap awards its PropExp (${trapExp}) modules`,
    );

    // --- Award via an EXP-cookie pickup (stack count) ---
    // Discover an ExpCookie pile carrying a positive stack count (its module
    // worth). Runtime ids aren't stable, so find it by script + StackCount.
    const expEntities = (await game.entities.list({ filter: "EXP" })).entities;
    let cookie: EntitySummary | undefined;
    let stack = 0;
    for (const e of expEntities) {
      const detail = await game.entities.detail(e.id);
      const isCookie = detail.properties.some(
        (p) => p.name === "Scripts" && p.value.includes("ExpCookie"),
      );
      const sc = detail.properties.find((p) => p.name === "StackCount");
      if (isCookie && sc && Number(sc.value) > 0) {
        cookie = e;
        stack = Number(sc.value);
        break;
      }
    }
    assert.ok(cookie, "medsci1 should have an ExpCookie pile with a stack count");

    await game.entities.sendMessage(cookie.id, { type: "Frob" });
    await game.step({ frames: 3 });
    const afterCookie = await modules(game);
    assert.equal(
      afterCookie,
      trapExp + stack,
      `frobbing the cookie awards its stack count (${stack}) on top of the trap`,
    );

    // --- Persists across save/load ---
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 3 });
    assert.equal(
      await modules(game),
      afterCookie,
      "the module balance survives save/load",
    );

    // --- Persists across a level transition ---
    await game.transitionLevel("eng1.mis");
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).mission,
      "eng1.mis",
      "transition reached eng1",
    );
    assert.equal(
      await modules(game),
      afterCookie,
      "the module balance survives a level transition",
    );
  },
);
