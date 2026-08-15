import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement } from "../src/types.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { fireOnce } from "./helpers/weapon.js";

// Honest Earth Psionic Training regression for #548. Runtime ids are
// discovered on every launch; positive template ids are stable mission object
// ids. Teleports only stage outside real sensors or at clear item/target
// sightlines. Entry, pickups, casting, inventory use, and exit all flow through
// production input and authored mission links.
//
// Negative-first on the campaign parent: entry remains at 40 psi because
// ReducePsi is unimplemented, and double-clicking a carried Psi Booster is a
// no-op because PsiKitScript is unimplemented.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const PSIONIC_ENTRY_TRIPWIRE = 320;
const PSIONIC_ENTRY_DESTINATION = 325;
const PSI_BOOSTERS = [275, 288, 289] as const;
const PSI_AMP = 290;
const TRAINING_DROID = 593;
const PSIONIC_EXIT_TRIPWIRE = 376;
const PSIONIC_EXIT_DESTINATION = 373;

function psiPoints(gameInfo: Awaited<ReturnType<GameServer["info"]>>): number {
  const psi = gameInfo.player.psi_points;
  assert.ok(psi !== null, "Earth player should have a psi pool");
  return psi;
}

async function exactlyOne(
  game: GameServer,
  templateId: number,
  label: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(templateId);
  assert.equal(
    matches.length,
    1,
    `expected exactly one ${label} (${templateId}), got ${JSON.stringify(matches)}`,
  );
  return matches[0];
}

async function boosterStripElement(
  game: GameServer,
  entityId: number,
): Promise<UiElement> {
  const ui = await game.ui.state();
  assert.equal(ui.mode, "use", "inventory use must happen in the live use-mode strip");
  const element = ui.strip?.elements.find((candidate) => candidate.entity_id === entityId);
  assert.ok(element, `use-mode strip should expose carried booster ${entityId}`);
  return element;
}

async function useBooster(game: GameServer, boosterId: number): Promise<void> {
  const element = await boosterStripElement(game, boosterId);
  await clickUiElement(game, element);
  assert.equal(
    (await game.ui.state()).cursor?.entity_id,
    boosterId,
    "first click should lift the real carried booster onto the cursor",
  );
  await clickUiElement(game, element);
  assert.equal(
    (await game.ui.state()).cursor,
    null,
    "second click should complete inventory use and release the cursor",
  );
}

/** Returns the target's live handle, which a load can renumber. */
async function aimAtEntity(
  game: GameServer,
  target: EntitySummary,
): Promise<EntitySummary> {
  const [tx, ty, tz] = (await game.entities.detail(target.id)).position;
  // Stand on the droid's authored platform, close enough that its torso is a
  // clear target. The old x-4 staging was unsupported void: the player kept
  // falling after aimAt computed its ray, so Cryokinesis hit level geometry
  // instead of the droid and falsely looked like a damage-path bug (#696).
  await game.player.teleport({ x: tx + 4, y: ty + 1, z: tz });
  await game.step({ frames: 60 });
  const supported = await game.player.position();
  await game.step({ frames: 10 });
  const stillSupported = await game.player.position();
  assert.ok(
    Math.abs(supported.y - stillSupported.y) < 0.02,
    `target staging must be collision-supported, got ${JSON.stringify({
      supported,
      stillSupported,
    })}`,
  );
  await game.input.set("left_hand.thumbstick", [0.75, 0]);
  await game.step({ frames: 20 });
  await game.input.set("left_hand.thumbstick", [0, 0]);
  await game.save("world-aim-rotated");
  await game.load("world-aim-rotated");
  const pawn = (await game.info()).player.rotation;
  assert.ok(
    Math.abs(pawn[1]) > 0.01 || Math.abs(pawn[3] - 1) > 0.01,
    `regression requires a non-identity save-restored pawn rotation: ${pawn}`,
  );
  // Loading re-instantiates the world, so a runtime entity id captured before
  // the save can name a different object afterwards - runtime ids are not a
  // stable identity (AGENTS.md), only the template id is. Aiming at the stale
  // number silently aimed at whatever inherited it: the "target" reported an
  // empty aim_point list hundreds of units away, so the aim fell back to
  // 'center' and the shot missed. Re-resolve through the template instead, and
  // hand the live handle back so the caller's damage check follows the same
  // object.
  const live = await exactlyOne(game, target.template_id, "Training Droid");
  const aim = await game.player.aimAt(live, {
    hitbox: "torso",
    visibility: "required",
  });
  assert.equal(aim.entity_id, live.id);
  assert.equal(aim.classification, "torso");
  assert.equal(aim.fallback_used, false);
  await game.step({ frames: 3 });
  return live;
}

function hitPoints(detail: Awaited<ReturnType<GameServer["entities"]["detail"]>>): number {
  const property = detail.properties.find((candidate) => candidate.name === "HitPoints");
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}

test(
  "VR world-frob consumes one Psi Booster without racing its automatic pickup",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8200),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    await crossEarthTrainingTripwire(
      game,
      PSIONIC_ENTRY_TRIPWIRE,
      PSIONIC_ENTRY_DESTINATION,
    );
    assert.equal(psiPoints(await game.info()), 5);

    const booster = await exactlyOne(game, PSI_BOOSTERS[0], "authored Psi Booster");
    const [x, y, z] = (await game.entities.detail(booster.id)).position;
    await teleportVerified(game, { x: x + 0.4, y: y + 1, z });
    const livePosition = (await game.entities.detail(booster.id)).position;
    const aim = await aimVrHandAt(game, livePosition, 0.25);
    const handHit = await game.raycast({
      start: aim.start,
      end: aim.target,
      collision_groups: ["entity", "selectable", "world", "raycast"],
      max_distance: 1,
      ignore_sensors: true,
    });
    assert.equal(
      handHit.entity_id,
      booster.id,
      `the production hand ray must hit the booster: ${JSON.stringify(handHit)}`,
    );

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });

    assert.equal(
      psiPoints(await game.info()),
      25,
      `the booster should restore exactly 20 psi; messages=${JSON.stringify(
        (await game.messages.recent()).messages.slice(-5),
      )}`,
    );
    assert.equal(
      (await game.entities.byTemplate(booster.template_id)).length,
      0,
      "the one-unit booster should be consumed exactly once",
    );
    assert.equal(
      (await game.player.inventory()).items.filter((item) => item.name === "Psi Booster")
        .length,
      0,
      "the stale automatic pickup must not add the consumed booster to the backpack",
    );
  },
);

test(
  "Earth Psionic Training reduces, refills, casts, and cleans up through real objectives",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8201),
    });
    await game.step({ frames: 5 });

    assert.equal(psiPoints(await game.info()), 40, "fresh Earth campaign psi");
    await crossEarthTrainingTripwire(
      game,
      PSIONIC_ENTRY_TRIPWIRE,
      PSIONIC_ENTRY_DESTINATION,
    );
    assert.equal(
      psiPoints(await game.info()),
      5,
      "the authored ReducePsi entry lesson should set psi to five",
    );

    const boosters: EntitySummary[] = [];
    for (const templateId of PSI_BOOSTERS) {
      const booster = await exactlyOne(game, templateId, "authored Psi Booster");
      await earthWorldUse(game, booster);
      assert.ok(
        (await game.player.inventory()).items.some(
          (item) => item.entity_id === booster.id,
        ),
        `normal world-use should physically carry booster ${templateId}`,
      );
      boosters.push(booster);
    }

    const amp = await exactlyOne(game, PSI_AMP, "authored Psi Amp");
    await earthWorldUse(game, amp);
    assert.equal(
      (await game.info()).player.wielded_entity_id,
      amp.id,
      "normal world-use should wield the authored Psi Amp",
    );

    // Exercise the walkthrough's explicit Y/CyclePsiPower lesson. A real
    // mission knows only the default OSA power, so the selection remains
    // Projected Cryokinesis and the HUD/API agree before the cast.
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.selected_psi_power, "Cryokinesis");

    const droid = await exactlyOne(game, TRAINING_DROID, "Training Droid");
    const droidHpBefore = hitPoints(await game.entities.detail(droid.id));
    const psiBeforeCast = psiPoints(await game.info());
    const liveDroid = await aimAtEntity(game, droid);
    await fireOnce(game);
    await game.step({ frames: 60 });
    assert.equal(
      psiPoints(await game.info()),
      psiBeforeCast - 1,
      "normal tier-one Cryokinesis cast should spend one psi",
    );
    assert.ok(
      hitPoints(await game.entities.detail(liveDroid.id)) < droidHpBefore,
      "normal Cryokinesis projectile should damage the real Training Droid",
    );

    // aimAtEntity deliberately exercises save/load; carried entities receive
    // fresh runtime ids on load just like the droid. Re-resolve the boosters
    // through their stable mission template ids before using them.
    const liveBoosters: EntitySummary[] = [];
    for (const booster of boosters) {
      liveBoosters.push(
        await exactlyOne(game, booster.template_id, "loaded Psi Booster"),
      );
    }

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    let expectedPsi = psiBeforeCast - 1;
    let expectedBoosters = liveBoosters.length;
    for (const booster of liveBoosters) {
      await useBooster(game, booster.id);
      expectedPsi = Math.min(expectedPsi + 20, 50);
      expectedBoosters -= 1;
      assert.equal(
        psiPoints(await game.info()),
        expectedPsi,
        "each Psi Booster restores 20 without exceeding the authored maximum",
      );
      assert.equal(
        (await game.entities.byTemplate(booster.template_id)).length,
        0,
        `inventory use must consume exactly the authored booster ${booster.template_id}`,
      );
      assert.equal(
        (await game.player.inventory()).items.filter(
          (item) => item.name === "Psi Booster",
        ).length,
        expectedBoosters,
      );
    }
    assert.equal(psiPoints(await game.info()), 50, "third booster clamps at max psi");

    await crossEarthTrainingTripwire(
      game,
      PSIONIC_EXIT_TRIPWIRE,
      PSIONIC_EXIT_DESTINATION,
    );
    assert.equal((await game.player.inventory()).count, 0);
    let player = (await game.info()).player;
    assert.equal(player.wielded_entity_id, null);
    assert.equal(player.right_hand_entity_id, null);
    assert.equal((await game.entities.byTemplate(PSI_AMP)).length, 0);

    // Reopen the strip after cleanup: API and the captured visible UI must both
    // show an actually-empty inventory, not a stale destroyed-item snapshot.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
    assert.equal((await game.ui.state()).mode, "shooter");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const reopened = await game.ui.state();
    assert.equal(reopened.mode, "use");
    assert.deepEqual(
      reopened.strip?.elements.filter((element) => element.entity_id !== null),
      [],
      "reopened strip may draw its frame but must expose no carried item",
    );
    await game.screenshot("earth-psionic-empty-inventory.png");

    player = (await game.info()).player;
    assert.equal(player.wielded_entity_id, null);
  },
);
