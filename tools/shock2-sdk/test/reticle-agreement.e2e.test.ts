import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { ammoOf, fireOnce } from "./helpers/weapon.js";

/** What the crosshair is advertising for the wielded weapon: the half-width of
 * the random error square and recoil's deflection, both in radians - the same
 * numbers the reticle is drawn from. */
async function reticle(
  game: GameServer,
  id: number,
): Promise<{
  spread_radians: number;
  bias_radians: [number, number];
  projectile_template: number | null;
}> {
  const detail = await game.entities.detail(id);
  const property = detail.properties.find((p) => p.name === "Reticle");
  assert.ok(property, "the wielded weapon should expose its reticle");
  return JSON.parse(property.value);
}

/** Angle between two directions, in radians. */
function angleBetween(a: number[], b: number[]): number {
  const dot = a[0]! * b[0]! + a[1]! * b[1]! + a[2]! * b[2]!;
  const mag = Math.hypot(...a) * Math.hypot(...b);
  return Math.acos(Math.min(1, Math.max(-1, dot / mag)));
}

const SHOTGUN = -19;
const PELLET_BOX = -42;
/** The bullet-impact marker the shot spawns where it lands. */
const IMPACT = -3544;

test(
  "the crosshair advertises the cone the pellets actually fly in",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 5 });
    // The 12m corridor the accuracy suite measures against.
    await game.player.teleport({ x: -34.96759, y: -4.7559557, z: 20.9 });
    await game.input.set("head.look", [-90, 0]);
    await game.step({ frames: 3 });
    await game.player.spawnItem(SHOTGUN);
    await game.player.spawnItem(PELLET_BOX);
    await game.player.setStats({ skills: { standard_weapons: 6 } });
    // Recoil is a separate, deterministic mechanic; this test is about the
    // random cone, so keep the shot on the crosshair (see flat-recoil-aim.e2e).
    await game.devParams.set("flat_recoil_aim", 0);
    await game.input.trigger("EquipShotgun");
    await game.step({ frames: 10 });
    const gun = (await game.info()).player.wielded_entity_id;
    assert.ok(gun);

    // The slug load is a single straight shot: the reticle must sit closed.
    const slug = await reticle(game, gun);
    assert.equal(slug.spread_radians, 0, "a slug advertises no cone");

    // Switch to the pellet load, which is the only stock ammo with spread.
    for (let i = 0; i < 4 && (await reticle(game, gun)).spread_radians === 0; i++) {
      await game.input.trigger("CycleAmmo");
      await game.step({ frames: 15 });
    }
    const advertised = (await reticle(game, gun)).spread_radians;
    assert.ok(
      advertised > 0.09 && advertised < 0.1,
      `the pellet load's authored 1024 units is 5.625 deg: got ${advertised}`,
    );

    await game.input.trigger("Reload");
    await game.step({ frames: 180 });
    assert.ok(ammoOf(await game.entities.detail(gun)) > 0, "shotgun should load pellets");

    // The camera axis the cone is drawn around.
    const aim = (await game.entities.detail(gun)).properties.find(
      (p) => p.name === "FlatAim",
    );
    assert.ok(aim);
    const { origin, forward } = JSON.parse(aim.value) as {
      origin: [number, number, number];
      forward: [number, number, number];
    };

    const deviations: number[] = [];
    for (let shell = 0; shell < 4; shell++) {
      if (ammoOf(await game.entities.detail(gun)) < 1) break;
      const prior = new Set((await game.entities.byTemplate(IMPACT)).map((e) => e.id));
      await fireOnce(game);
      await game.step({ frames: 20 });
      for (const hit of await game.entities.byTemplate(IMPACT)) {
        if (prior.has(hit.id) || !hit.position) continue;
        deviations.push(
          angleBetween(
            [
              hit.position[0]! - origin[0],
              hit.position[1]! - origin[1],
              hit.position[2]! - origin[2],
            ],
            forward,
          ),
        );
      }
    }

    assert.ok(deviations.length >= 6, `expected pellet impacts, got ${deviations.length}`);
    // Heading and pitch are drawn independently, so the distribution is a
    // SQUARE of half-width `advertised` - its corner is sqrt(2) times as far
    // from the axis as its edge. The arms mark the edge, so the honest bound
    // on a single pellet is the corner.
    const bound = advertised * Math.SQRT2;
    const worst = Math.max(...deviations);
    assert.ok(
      worst <= bound + 1e-3,
      `a pellet flew outside the advertised cone: ${worst} > ${bound}`,
    );
    // ...and the reticle must not pass by being uselessly wide: with this many
    // pellets the distribution should reach a good fraction of its own edge.
    assert.ok(
      worst >= advertised * 0.5,
      `the reticle is far wider than the real spread: ${worst} vs ${advertised}`,
    );
  },
);
