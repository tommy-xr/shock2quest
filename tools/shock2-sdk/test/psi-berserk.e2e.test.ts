import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { pullTrigger } from "./helpers/weapon.js";

// Adrenaline Overproduction ("Berserk" in the gamesys, tier 2 sustained):
// while it is active the player's melee hits are scaled by its authored
// data[0] (+13%) and the adrenaline drains data[1] (1 HP) a second, for
// duration_base + duration_per_psi x PSI = 0 + 10*5 = 50 seconds.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const WRENCH = -928;
const PSI_AMP = -247;
const BERSERK_COST = 2;
/** Flat melee resolves its hit at the swing motion's authored event, 41
 * simulation frames after the trigger edge (see flat-melee-animation-event). */
const SWING_FRAMES = 60;

function propOf(detail: EntityDetailResult, name: string): number {
  const property = detail.properties.find((candidate) => candidate.name === name);
  assert.ok(property, `entity ${detail.entity_id} should expose ${name}`);
  return Number(property.value);
}

/** The nearest creature with live hitboxes - the melee bench's target row. */
async function nearestCreature(game: GameServer): Promise<EntityDetailResult> {
  for (const entity of (await game.entities.list({ limit: 300 })).entities) {
    const detail = await game.entities.detail(entity.id);
    if ((detail.aim_points ?? []).length > 1) return detail;
  }
  assert.fail("expected a creature with hitboxes in debug_melee");
}

/** Provision and wield the authored player Wrench through production selection. */
async function wieldWrench(game: GameServer): Promise<void> {
  await game.input.trigger("EquipWrench");
  await game.step({ frames: 5 });
}

/** One flat swing at `target`, returning the hit points it cost the victim. */
async function swingOnce(game: GameServer, target: EntityDetailResult): Promise<number> {
  const [x, y, z] = (await game.entities.detail(target.entity_id)).position;
  await game.player.teleport({ x: x + 1.1, y: y + 1, z });
  await game.step({ frames: 30 });
  const aim = await game.player.aimAt(target.entity_id, {
    hitbox: "torso",
    visibility: "required",
  });
  assert.equal(aim.entity_id, target.entity_id, "the crosshair should be on the creature");

  const before = propOf(await game.entities.detail(target.entity_id), "HitPoints");
  await pullTrigger(game);
  await game.step({ frames: SWING_FRAMES });
  const after = propOf(await game.entities.detail(target.entity_id), "HitPoints");
  return before - after;
}

/** Cycle the psi selection onto Berserk and cast it (bounded). */
async function castBerserk(game: GameServer): Promise<void> {
  // Unlike debug_psi, this bench stocks no amp - provision one and wield it
  // through the same production selection the player uses.
  await game.player.spawnItem(PSI_AMP);
  await game.input.trigger("EquipPsiAmp");
  await game.step({ frames: 10 });
  assert.ok(
    (await game.info()).player.wielded_entity_id !== null,
    "the psi amp should be wielded before casting",
  );
  for (let attempt = 0; attempt < 40; attempt += 1) {
    if ((await game.info()).player.selected_psi_power === "Berserk") break;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 1 });
  }
  const psiBefore = (await game.info()).player.psi_points;
  assert.equal(
    (await game.info()).player.selected_psi_power,
    "Berserk",
    "could not cycle the psi power selection to Berserk",
  );
  await pullTrigger(game);
  await game.step({ frames: 10 });
  const player = (await game.info()).player;
  assert.deepEqual(player.active_psi_powers, ["Berserk"], "Berserk should be active");
  assert.equal(player.psi_points, psiBefore! - BERSERK_COST, "the cast costs 2 psi points");
}

test(
  "Adrenaline Overproduction scales the player's melee damage",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Baseline: one unbuffed swing, in its own runtime so the creature is
    // fresh and nothing else has touched it.
    let baseline: number;
    {
      await using game = await GameServer.launch({ mission: "debug_melee" });
      await game.step({ frames: 20 });
      await game.player.spawnItem(WRENCH);
      await wieldWrench(game);
      baseline = await swingOnce(game, await nearestCreature(game));
      assert.ok(baseline > 0, "the unbuffed swing should damage the creature");
    }

    // ...and the same swing with Berserk active costs the creature more.
    {
      await using game = await GameServer.launch({ mission: "debug_melee" });
      await game.step({ frames: 20 });
      await game.player.spawnItem(WRENCH);
      await castBerserk(game);
      await wieldWrench(game);
      const buffed = await swingOnce(game, await nearestCreature(game));
      assert.deepEqual(
        (await game.info()).player.active_psi_powers,
        ["Berserk"],
        "Berserk should still be running when the blow lands",
      );
      // Hit points are integers, so the +13% shows up as the rounded blow:
      // 6 -> 7 for the Wrench. The load-bearing assertion is that it is bigger.
      assert.ok(
        buffed > baseline,
        `a berserk swing should hurt more than ${baseline}; got ${buffed}`,
      );
      assert.equal(
        buffed,
        Math.round(baseline * 1.13),
        `the blow should scale by the authored +13% (baseline ${baseline})`,
      );
    }
  },
);

test(
  "Adrenaline Overproduction drains the caster's health while it runs",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 20 });
    // Cast from the spawn bay, out of the penned creatures' reach: the only
    // thing that may touch the player's health here is the adrenaline.
    await castBerserk(game);
    const before = (await game.info()).player.hit_points;
    assert.ok(before !== null && before > 5);

    // 300 frames = 5 seconds = 5 HP at the authored 1 HP/s.
    await game.step({ frames: 300 });
    const after = (await game.info()).player.hit_points;
    assert.equal(after, before! - 5, "5 seconds of Berserk should cost 5 hit points");
    assert.deepEqual(
      (await game.info()).player.active_psi_powers,
      ["Berserk"],
      "the 50 s power is still running after 5 s",
    );
  },
);
