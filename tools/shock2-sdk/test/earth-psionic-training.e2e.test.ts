import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
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

// Negative-first production-VR regression for #958. On the parent commit the
// squeeze really holds mission object 275, but the holder trigger sends only
// TriggerPull. PsiKitScript never sees its authored inventory Frob, leaving PSI
// at five and the exact held entity alive. This test intentionally does not use
// a direct Frob, Give, inventory-strip click, or debug vitals mutation.
test(
  "VR uses the actual held Earth Psi Booster through its authored inventory action",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    await crossEarthTrainingTripwire(
      game,
      PSIONIC_ENTRY_TRIPWIRE,
      PSIONIC_ENTRY_DESTINATION,
    );
    assert.equal(
      psiPoints(await game.info()),
      5,
      "the held-consumable regression starts from the authored course value",
    );

    const booster = await exactlyOne(game, PSI_BOOSTERS[0], "authored Psi Booster");
    const boosterBodies = (await game.physics.bodies({ entityId: booster.id })).bodies;
    assert.equal(boosterBodies.length, 1, "booster 275 must begin as one physical world item");
    // MOVE pickups are dynamically instantiated one sixth of a Dark unit
    // above PropPosition. Aim at the live body, not the authored transform;
    // aiming at the latter passes underneath this tiny hypo collider.
    const boosterPosition = boosterBodies[0].position;
    // The three boosters are only 0.4 units apart along x. Stand on object
    // 275's lower-x side so it is the first selectable body on this ray.
    await teleportVerified(game, {
      x: boosterPosition[0] - 1.5,
      y: boosterPosition[1] - 0.42,
      z: boosterPosition[2],
    });
    const handRay = await aimVrHandAt(game, boosterPosition, 0.15);
    const hit = await game.raycast({
      start: handRay.start,
      end: handRay.target,
      collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
      max_distance: 1,
      ignore_sensors: true,
    });
    assert.equal(
      hit.entity_id,
      booster.id,
      `the production hand ray must select booster 275: ${JSON.stringify({ booster, boosterBodies, handRay, hit, player: (await game.info()).player.position })}`,
    );

    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    const heldBeforeUse = await game.info();
    assert.equal(
      heldBeforeUse.player.right_hand_entity_id,
      booster.id,
      "the regression must use the actual entity held by the right VR hand",
    );
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === booster.id && item.location === "right_hand",
      ),
      "the exact held booster must be part of the player's carried state",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: booster.id })).bodies.length,
      0,
      "a genuinely held item must already be outside world-ray selection",
    );

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });

    const afterUse = await game.info();
    const messageTrace = await game.messages.recent();
    const heldItemMessages = messageTrace.messages.filter(
      (message) => message.to.entity_id === booster.id,
    );
    assert.ok(
      heldItemMessages.some((message) => message.payload === "Frob"),
      "the holder trigger must route the authored inventory Frob to the exact held booster",
    );
    assert.ok(
      !heldItemMessages.some((message) => message.payload === "TriggerPull"),
      "a held consumable must not receive the weapon TriggerPull protocol",
    );
    assert.equal(
      psiPoints(afterUse),
      25,
      "one holder-trigger gesture must restore exactly the retail 20 PSI",
    );
    assert.equal(
      (await game.entities.byTemplate(booster.template_id)).length,
      0,
      "using the one-unit mission object must consume that exact held entity",
    );
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      null,
      "canonical entity teardown must release the hand after consumption",
    );
    assert.ok(
      !(await game.player.inventory()).items.some((item) => item.entity_id === booster.id),
      "the consumed held id must leave all carried locations",
    );
    for (const remainingTemplate of PSI_BOOSTERS.slice(1)) {
      assert.equal(
        (await game.entities.byTemplate(remainingTemplate)).length,
        1,
        "using one held unit must not consume either neighboring course booster",
      );
    }

    // Continue the room's authored VR lesson. This also proves that the
    // generic consumable decision did not steal TriggerPull from the Psi Amp.
    await game.input.set("right_hand.trigger", 0);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });

    const amp = await exactlyOne(game, PSI_AMP, "authored Psi Amp");
    const ampBodies = (await game.physics.bodies({ entityId: amp.id })).bodies;
    assert.equal(ampBodies.length, 1, "the authored Psi Amp should be physically supplied");
    const ampPosition = ampBodies[0].position;
    await teleportVerified(game, {
      x: ampPosition[0] - 1.5,
      y: ampPosition[1] - 0.42,
      z: ampPosition[2],
    });
    await aimVrHandAt(game, ampPosition, 0.18);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      amp.id,
      "the production hand must hold the course's actual Psi Amp",
    );

    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.selected_psi_power, "Cryokinesis");

    const droid = await exactlyOne(game, TRAINING_DROID, "Training Droid");
    const droidHpBefore = hitPoints(await game.entities.detail(droid.id));
    const droidPosition = (await game.entities.detail(droid.id)).position;
    await teleportVerified(game, {
      x: droidPosition[0] - 4,
      y: droidPosition[1],
      z: droidPosition[2],
    });
    const droidAim = await game.player.aimAt(droid, {
      hitbox: "torso",
      // The VR projectile starts at the hand, not the camera. Stage the hand
      // directly in front of the live torso below and let that production ray
      // be the authoritative clearance check.
      visibility: "unchecked",
    });
    await aimVrHandAt(game, droidAim.world_point, 0.45, 1);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 61 });
    assert.equal(
      psiPoints(await game.info()),
      24,
      "the held Psi Amp must preserve TriggerPull and spend one PSI on Cryokinesis",
    );
    assert.ok(
      hitPoints(await game.entities.detail(droid.id)) < droidHpBefore,
      "the real held-Amp cast must damage the course's Training Droid",
    );

    await crossEarthTrainingTripwire(
      game,
      PSIONIC_EXIT_TRIPWIRE,
      PSIONIC_EXIT_DESTINATION,
    );
    const exited = await game.info();
    assert.equal(exited.player.right_hand_entity_id, null);
    assert.equal((await game.player.inventory()).count, 0);
    assert.equal((await game.entities.byTemplate(PSI_AMP)).length, 0);
    assert.equal((await game.entities.byTemplate(PSI_BOOSTERS[0])).length, 0);
    for (const templateId of PSI_BOOSTERS.slice(1)) {
      assert.equal((await game.entities.byTemplate(templateId)).length, 1);
    }
  },
);

test(
  "VR world-frob consumes one Psi Booster without racing its automatic pickup",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
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
      // Authored SPHERE physics leaves the nearby booth floor, rather than a
      // synthetic model-bounds box, supporting the player. Let that placement
      // resolve before deriving the production crosshair ray.
      await earthWorldUse(game, booster, {
        horizontalOffset: 0.1,
        verticalOffset: -PLAYER_EYE_HEIGHT_WORLD,
        settleFrames: 1,
      });
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
