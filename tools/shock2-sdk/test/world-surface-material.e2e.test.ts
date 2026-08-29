import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { describeSounds as describe, tagValue } from "./helpers/audio.js";

// World geometry carries a material now: each level trimesh triangle knows the
// material of the texture it was built from (from that texture's `t_fam`
// archetype), so a ray hit and a footstep name the deck they landed on instead
// of the blanket "metal" every surface used to report.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** A grid of downward probes over MedSci, returning the materials found. */
async function floorMaterials(game: GameServer): Promise<Map<string, number>> {
  const found = new Map<string, number>();
  for (let x = -40; x <= 40; x += 10) {
    for (let z = -30; z <= 40; z += 10) {
      for (const y of [0, -4]) {
        const hit = await game.raycast({
          start: [x, y + 3, z],
          end: [x, y - 4, z],
          collision_groups: ["world"],
        });
        const material = hit.hit ? hit.surface_material : undefined;
        if (material) {
          found.set(material, (found.get(material) ?? 0) + 1);
        }
      }
    }
  }
  return found;
}

test(
  "world geometry reports the material of the surface hit",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 60 });

    const materials = await floorMaterials(game);
    assert.ok(
      materials.size > 1,
      `MedSci's decks are not one material: got ${JSON.stringify([...materials])}`,
    );
    // Both of these are authored on medsci1's textures; if archetype
    // inheritance ever stopped resolving the metaproperty's value, every
    // surface would collapse to the base archetype's "plasticrete" instead.
    assert.ok(
      materials.has("plasticrete") && materials.has("metal"),
      `expected plasticrete deck and metal plate: ${JSON.stringify([...materials])}`,
    );

    // The creatures wandering MedSci plant their feet on those same decks, and
    // the footstep schema keys on what they are standing on: the deck here is
    // plasticrete (`ft_og*`), not the metal (`ft_ogm*`) every step used to
    // resolve.
    await game.step({ frames: 240 });
    const footsteps = (await game.audio.recent()).sounds.filter(
      (sound) => tagValue(sound, "event") === "footstep",
    );
    assert.ok(
      footsteps.length > 0,
      "expected MedSci's creatures to be walking around",
    );
    assert.ok(
      footsteps.some((sound) => tagValue(sound, "material2") !== "metal"),
      `footsteps should name the deck underfoot: ${describe(footsteps)}`,
    );
  },
);
