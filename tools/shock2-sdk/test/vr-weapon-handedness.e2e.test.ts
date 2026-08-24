import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { SceneObjectSummary, Vec3 } from "../src/types.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// End-to-end regression for weapon handedness in VR.
//
// `PlayerInfo` has two hand slots. Flatscreen wields into the LEFT one and
// never fills the right, so reload, ammo cycling and every ammo readout used
// to read `left_hand_entity_id` directly and were correct by construction. In
// VR the slots are the two literal motion controllers, so a gun grabbed with
// the RIGHT hand was invisible to all of it: Reload did nothing, CycleAmmo did
// nothing, and the right forearm panel stayed a static AMMOFULL quad.
//
// Every assertion below runs with the pistol in the RIGHT hand and the left
// hand empty, which is exactly the case the old left-slot-only code missed -
// so this test fails wholesale before `shock2vr::wielded_weapon`.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

const distance = (a: Vec3, b: Vec3): number =>
  Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);

/**
 * The draws that landed on one world-space panel.
 *
 * A panel is a `UiCanvas` rendered by `render_world_space`, which emits one
 * scene object per canvas element, all sharing the panel's transform apart
 * from a millimetre-scale depth step that layers them. So a panel is a tight
 * cluster of `player_hands` draws, and its size is its element count; the
 * nearest thing that is not part of it (the arm mesh) is ~0.4 m away.
 */
function drawsAt(objects: SceneObjectSummary[], panel: Vec3): SceneObjectSummary[] {
  return objects.filter((object) => distance(object.position, panel) < 0.02);
}

/**
 * Locate the right forearm's ammo panel: the biggest cluster of `player_hands`
 * draws worn near the right hand (the hand/arm meshes and the left BIOFULL
 * panel sit elsewhere). Only valid while the readout has something to say -
 * the point of calling it is to pin the panel's origin, which stays put as
 * long as the hand pose does.
 */
async function locateForearmPanel(game: GameServer, rightHand: Vec3): Promise<Vec3> {
  const near = (await game.scene.fromSource("player_hands")).filter(
    (object) => distance(object.position, rightHand) < 1.0,
  );
  const best = near
    .map((object) => object.position)
    .sort((a, b) => drawsAt(near, b).length - drawsAt(near, a).length)[0];
  assert.ok(best, "the right forearm should be wearing a panel");
  return best;
}

/** How many elements the right forearm's ammo panel drew this frame. */
async function forearmAmmoDraws(game: GameServer, panel: Vec3): Promise<number> {
  return drawsAt(await game.scene.fromSource("player_hands"), panel).length;
}

test(
  "VR: a weapon in the right hand reloads, cycles ammo and drives the forearm readout",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });

    // DebugCycleWeapon spawns the pistol; the VR path has no auto-wield, so it
    // drops to the floor for the hands to pick up.
    await game.step({ frames: 10 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 90 });
    const pistol = (await game.entities.list({ limit: 200 })).entities.find(
      (entity) => entity.name === "Pistol",
    );
    assert.ok(pistol, "pistol should have spawned");

    // Reserve rounds for the reload. `spawn-item` enters the backpack in both
    // presentations, so this needs no VR inventory interaction.
    await game.player.spawnItem("Small Standard Clip");
    await game.step({ frames: 2 });

    // Grab it with the RIGHT hand, leaving the left (the flat wield slot) empty.
    // The hand pose is unchanged from here on (only the grip opens and closes),
    // so the forearm panel's transform - and the cluster of draws that
    // identifies it - stays put for the rest of the test.
    const { start: rightHand } = await aimVrHandAt(game, pistol.position, 0.45, 1);
    await game.step({ frames: 8 });
    const armed = (await game.info()).player;
    assert.equal(armed.right_hand_entity_id, pistol.id, "pistol is in the right hand");
    assert.equal(armed.wielded_entity_id, null, "the left (flat wield) slot stays empty");

    // 1. The readout resolves the right hand's gun.
    assert.equal(
      armed.wielded_ammo_type,
      "std",
      "the ammo readout sees a right-hand gun (was null: left slot only)",
    );

    // 2. The forearm panel composites the live readout on the backdrop: round
    //    count + ammo icon + type label. Before the fix the readout resolved to
    //    nothing and only the backdrop drew.
    const panel = await locateForearmPanel(game, rightHand);
    assert.equal(
      await forearmAmmoDraws(game, panel),
      4,
      "backdrop + round count + ammo icon + type label",
    );

    // 3. The readout is live: what it reports tracks the clip as rounds are
    //    consumed (the panel's text content itself is asserted without a GL
    //    context by `hud::ammo_panel`'s unit tests).
    const loaded = ammoOf(await game.entities.detail(pistol.id));
    assert.ok(loaded > 3, "debug pistol starts loaded");
    for (let i = 0; i < 3; i++) {
      await game.input.set("right_hand.trigger", 1.0);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0.0);
      await game.step({ frames: 10 });
    }
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      loaded - 3,
      "three right-hand shots consumed three rounds",
    );
    assert.equal(
      await forearmAmmoDraws(game, panel),
      4,
      "the readout stays on the forearm while firing",
    );

    // 4. Reload finds the right hand's gun and fills it from the reserve clip.
    await game.input.trigger("Reload");
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.reloading,
      true,
      "Reload starts on the right hand's weapon (was a no-op)",
    );
    await game.step({ frames: 150 });
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      loaded,
      "the reserve clip refilled the right hand's magazine",
    );

    // 5. CycleAmmo likewise. Cycling needs an empty magazine (a loaded one has
    //    an established projectile identity), so empty it first.
    for (let i = 0; i < loaded; i++) {
      await game.input.set("right_hand.trigger", 1.0);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0.0);
      await game.step({ frames: 10 });
    }
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), 0, "magazine emptied");
    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      // The debug pistol's authored projectile links, in order: std -> ap.
      "ap",
      "CycleAmmo advances the right hand's weapon to its next type (was a no-op)",
    );

    // 6. Dropping it puts the forearm back to the bare backdrop.
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.right_hand_entity_id, null, "pistol dropped");
    assert.equal(
      await forearmAmmoDraws(game, panel),
      1,
      "an empty hand leaves the bare AMMOFULL backdrop",
    );
  },
);
