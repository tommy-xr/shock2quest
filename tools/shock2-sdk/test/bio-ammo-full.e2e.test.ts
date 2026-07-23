import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test for the BIOFULL/AMMOFULL expanded readouts in use mode
// (flat UI 5, projects/flat-ui.md §1.2 / §6 PR 5):
//
//   - Entering use mode (Tab) expands the compact bottom readouts: BIO.PCX ->
//     BIOFULL.PCX (bottom-left) and AMMOBACK -> AMMOFULL.PCX (bottom-right).
//   - With a gun that has 2+ ammo types wielded, AMMOFULL shows an ammo-type
//     CYCLE button while the magazine is empty, exposed via /v1/ui
//     `ammo_cycle`. Clicking it cycles the wielded weapon's ammo type
//     (Effect::CycleAmmo).
//   - Shooter mode keeps the compact readouts (no ammo_cycle element).
//
// Negative-first: on the base branch (flat UI 4.5) there is no AMMOFULL cycle
// button and /v1/ui has no `ammo_cycle` field, so the KEY assertion ("use mode
// exposes the ammo-cycle button with a gun wielded") fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The Pistol (gamesys template -17) has 3 ammo types (Standard / HE / AP).

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

test(
  "use mode exposes AMMOFULL ammo-cycle for an empty multi-ammo weapon",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8159),
    });
    await game.step({ frames: 5 });

    const clickScreen = async (rect: [number, number, number, number]) => {
      const [x, y, w, h] = rect;
      await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 1);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 2 });
    };

    // --- Setup: give the player a Pistol and wield it ---
    const { entities: pistols } = await game.entities.list({ filter: "Pistol", limit: 20 });
    const pistol = pistols.find((e) => (e.name ?? "") === "Pistol");
    assert.ok(pistol, `expected a Pistol in medsci1 (got ${JSON.stringify(pistols.map((e) => e.name))})`);
    await game.player.give(pistol.id);

    // Shooter baseline: no ammo_cycle element (compact readouts).
    const shooter = await game.ui.state();
    assert.equal(shooter.mode, "shooter");
    assert.ok(!shooter.ammo_cycle, "shooter mode must not expose the ammo-cycle button");
    await game.screenshot("ammo-shooter-compact.png");

    // Tab into use mode and wield the Pistol from the strip (double-click).
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const pistolEl = (await game.ui.state()).strip?.elements.find(
      (e) => e.kind === "button" && e.label === "Pistol",
    );
    assert.ok(pistolEl, "the Pistol should be in the strip");
    await clickScreen(pistolEl.screen_rect); // first click lifts...
    await clickScreen(pistolEl.screen_rect); // ...double-click wields
    await game.step({ frames: 5 });

    const wielded = await game.info();
    assert.ok(
      wielded.player.wielded_ammo_type,
      `the Pistol should be wielded with an ammo type (got ${JSON.stringify(wielded.player)})`,
    );

    // Loaded rounds have an established projectile identity, so drain the
    // magazine before asking the UI to expose its ammo-type cycle control.
    const loaded = ammoOf(await game.entities.detail(pistol.id));
    if (loaded > 0) {
      assert.ok(
        !(await game.ui.state()).ammo_cycle,
        "loaded magazine must not expose a conversion control",
      );
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
      for (let i = 0; i < loaded; i++) await fireOnce(game);
      assert.equal(ammoOf(await game.entities.detail(pistol.id)), 0);
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
    }

    // --- KEY: use mode exposes the AMMOFULL ammo-cycle button when empty ---
    const use = await game.ui.state();
    assert.equal(use.mode, "use");
    assert.ok(
      use.ammo_cycle,
      `use mode with a multi-ammo gun must expose the ammo-cycle button (got ${JSON.stringify(use)})`,
    );
    await game.screenshot("ammo-use-expanded.png");

    // --- Clicking the ammo-cycle button cycles the wielded ammo type ---
    const before = (await game.info()).player.wielded_ammo_type;
    await clickScreen(use.ammo_cycle.screen_rect);
    await game.step({ frames: 3 });
    const after = (await game.info()).player.wielded_ammo_type;
    assert.notEqual(
      after,
      before,
      `clicking the ammo-cycle button should change the wielded ammo type (${before} -> ${after})`,
    );
    await game.screenshot("ammo-use-cycled.png");

    // --- Tab back to shooter: compact readouts, no ammo_cycle ---
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const restored = await game.ui.state();
    assert.equal(restored.mode, "shooter");
    assert.ok(!restored.ammo_cycle, "leaving use mode drops the ammo-cycle button");
    await game.screenshot("ammo-shooter-after.png");
  },
);
