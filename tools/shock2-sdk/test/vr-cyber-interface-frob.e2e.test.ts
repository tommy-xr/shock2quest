import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import { stackCount } from "./helpers/nanites.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// The VR trigger is overloaded: for a hand holding a weapon it is fire, and
// for an empty hand it is the universal world interact (Frob). Safing the
// weapon while the cyber interface is up used to zero BOTH hands' trigger,
// which took the interact gesture with it - with the panel open the player
// could not frob anything in the world. The mask is now per hand, so a hand
// that is neither pointing at the panel nor holding a weapon still frobs.
//
// Negative-first: on the parent this fails at the "collected" assertion -
// the pile is still sitting there with the stat unchanged.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8577);

/** Stable earth.mis mission object: a "Big Nanite Pile" (see
 * nanite-player-stat.e2e.test.ts). Collecting it moves a known amount into
 * `player.stats.nanites`, which makes the frob cleanly observable. */
const NANITE_PILE_OBJ = 257;
/** A second pile, so the latch scenario below starts from an uncollected one. */
const LATCH_PILE_OBJ = 292;

test(
  "an off-panel hand still frobs the world while the VR cyber interface is open",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const [pile] = await game.entities.byTemplate(NANITE_PILE_OBJ);
    assert.ok(pile, `expected earth mission object ${NANITE_PILE_OBJ} (Big Nanite Pile)`);
    const detail = await game.entities.detail(pile.id);
    const stack = stackCount(detail.properties);
    assert.ok(stack !== undefined && stack > 0, "expected a positive authored stack count");
    assert.equal(
      (await game.info()).player.stats?.nanites ?? 0,
      0,
      "a fresh earth character starts with no stat nanites",
    );

    // Stand over the pile and aim the production hand ray down at it. The
    // interface panel hangs at head height along the head's yaw (it is
    // gravity-aligned and yaw-only), so a steeply downward ray is genuinely
    // off the panel - which the pointer assertion below confirms rather than
    // assumes.
    const [x, y, z] = detail.position;
    await game.player.teleport({ x, y: y - PLAYER_EYE_HEIGHT_WORLD, z: z + 0.3 });
    await game.step({ frames: 5 });
    await aimVrHandAt(game, detail.position);

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    assert.equal((await game.ui.state()).mode, "use", "the cyber interface must be open");

    // Re-aim after the panel is placed, then prove the hand is off-panel: no
    // hand owns the canvas pointer, so this trigger is not a UI click.
    await aimVrHandAt(game, detail.position);
    assert.equal(
      (await game.ui.state()).pointer?.hand ?? null,
      null,
      "the hand must be off the panel for this to be the world-interact case",
    );

    // The interact gesture: a trigger pull from the empty, off-panel hand.
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    assert.equal(
      (await game.info()).player.stats?.nanites ?? 0,
      stack,
      "an off-panel trigger must still frob the world while the interface is open",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: pile.id })).bodies.length,
      0,
      "the collected pile should no longer have a world body",
    );
    // The frob must not have disturbed the interface itself.
    assert.equal(
      (await game.ui.state()).mode,
      "use",
      "frobbing the world must leave the cyber interface open",
    );
  },
);

test(
  "a trigger pull begun on the cyber-interface panel cannot slide off into a world frob",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const [pile] = await game.entities.byTemplate(LATCH_PILE_OBJ);
    assert.ok(pile, `expected earth mission object ${LATCH_PILE_OBJ} (Big Nanite Pile)`);
    const detail = await game.entities.detail(pile.id);
    const stack = stackCount(detail.properties);
    assert.ok(stack !== undefined && stack > 0, "expected a positive authored stack count");

    const [x, y, z] = detail.position;
    await game.player.teleport({ x, y: y - PLAYER_EYE_HEIGHT_WORLD, z: z + 0.3 });
    await game.step({ frames: 5 });

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const panel = (await game.ui.state()).panel_pose;
    assert.ok(panel, "the cyber interface must expose its panel pose");

    // Begin the pull ON the panel - this is a UI click, not a world interact.
    await aimVrHandAtCanvas(game, panel, [320, 240], { trigger: 1 });
    await game.step({ frames: 5 });
    assert.ok(
      (await game.ui.state()).pointer?.hand,
      "the pull must start with the hand owning the canvas pointer",
    );

    // Slide the ray off the panel onto the pile WITHOUT releasing. The panel is
    // head-anchored, so this happens in ordinary play from a head turn alone.
    // The gesture keeps the meaning it started with: it must not become a Frob.
    await aimVrHandAt(game, detail.position, 0.45, 0, 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.stats?.nanites ?? 0,
      0,
      "a click begun on the panel must not turn into a world frob when the ray slides off",
    );

    // Release and pull again, now genuinely off-panel: the latch has dropped
    // and this fresh gesture is the world's, so the pile collects. Without
    // this the assertion above could pass for the wrong reason.
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.stats?.nanites ?? 0,
      stack,
      "a fresh pull off the panel must still frob the world",
    );
  },
);
