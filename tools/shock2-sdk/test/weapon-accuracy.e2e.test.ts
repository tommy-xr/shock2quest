import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { ammoOf, fireOnce } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
// Optional private data fixture: only SKILLPARAM's first u16 is changed from
// stock0 to128 (0.703125 degrees per missing skill). Never modify shipped assets.
const fixtureRoot = process.env.SHOCK2_ACCURACY_FIXTURE;
for (const synthetic of [false, true])
  for (const strength of [1, 6]) {
    test(
      `${synthetic ? "synthetic nonzero data" : "stock data"} accuracy at Strength ${strength}`,
      {
        skip: !enabled || (synthetic && !fixtureRoot),
        timeout: 240_000,
      },
      async () => {
        const originalRoot = process.env.DARK_ASSET_PATH;
        try {
          if (synthetic) process.env.DARK_ASSET_PATH = fixtureRoot;
          await using game = await GameServer.launch({
            mission: "medsci1.mis",
          });
          await game.step({ frames: 5 });
          await game.player.setStats({ strength });
          await game.player.teleport({ x: -34.96759, y: -4.7559557, z: 20.9 });
          await game.input.set("head.look", [-90, 0]);
          await game.step({ frames: 3 });
          await game.player.spawnItem("Pistol");
          await game.player.spawnItem("Assault Rifle");
          // The first ray breaks a glass pane in this corridor. Exclude that
          // setup shot so every measured bullet reaches the same 12m wall.
          await game.player.setStats({ skills: { standard_weapons: 1 } });
          await game.input.trigger("EquipPistol");
          await game.step({ frames: 3 });
          await fireOnce(game);
          await game.step({ frames: 20 });
          for (const skill of [1, 3, 6]) {
            const stats = await game.player.setStats({
              skills: { standard_weapons: skill },
            });
            assert.equal(stats.strength, strength);
            assert.equal(stats.skills.standard_weapons, skill);
            for (const weapon of ["pistol", "ar"]) {
              await game.input.trigger(
                weapon === "pistol" ? "EquipPistol" : "EquipAssaultRifle",
              );
              await game.step({ frames: 3 });
              const id = (await game.info()).player.wielded_entity_id;
              assert.ok(id);
              const positions: number[][] = [];
              const refused = weapon === "ar" && skill < 6;
              for (let shot = 0; shot < (refused ? 1 : 8); shot++) {
                if (ammoOf(await game.entities.detail(id)) < 3) {
                  await game.player.spawnItem(-31);
                  await game.input.trigger("Reload");
                  await game.step({ frames: 180 });
                }
                const ammo = ammoOf(await game.entities.detail(id));
                const prior = new Set(
                  (await game.entities.byTemplate(-3544)).map((e) => e.id),
                );
                await fireOnce(game);
                await game.step({ frames: 20 });
                const hits = (await game.entities.byTemplate(-3544)).filter(
                  (e) => !prior.has(e.id),
                );
                const spent: number =
                  ammo - ammoOf(await game.entities.detail(id));
                if (refused) {
                  assert.equal(
                    spent,
                    0,
                    "under-skilled AR attempts are refusals, not accuracy samples",
                  );
                  assert.equal(hits.length, 0);
                  assert.ok(
                    (await game.entities.detail(id)).properties.some(
                      (p) => p.name === "WeaponSkillNotice",
                    ),
                    "the AR must report its unmet skill requirement",
                  );
                } else {
                  assert.equal(spent, 1);
                  assert.equal(
                    hits.length,
                    1,
                    "one actual bullet must reach the test wall",
                  );
                  assert.ok(
                    Math.abs(hits[0]!.position[2] - 32.9) < 0.02,
                    "the wall must receive the shot",
                  );
                  positions.push(hits[0]!.position);
                }
              }
              if (refused) continue;
              const width =
                Math.max(...positions.map((p) => p[0]!)) -
                Math.min(...positions.map((p) => p[0]!));
              const height =
                Math.max(...positions.map((p) => p[1]!)) -
                Math.min(...positions.map((p) => p[1]!));
              if (!synthetic || skill === 6) {
                assert.ok(
                  width < 0.002 && height < 0.002,
                  "stock/max-skill shots add no random aim error",
                );
              } else {
                assert.ok(
                  width > 0.01 && height > 0.01,
                  "nonzero data must produce independent two-axis spread",
                );
                const bound =
                  2 *
                    12.1 *
                    Math.tan((0.703125 * (6 - skill) * Math.PI) / 180) +
                  0.02;
                assert.ok(
                  width <= bound && height <= bound,
                  "skill controls the authored angular bounds",
                );
              }
            }
          }
        } finally {
          if (originalRoot === undefined) delete process.env.DARK_ASSET_PATH;
          else process.env.DARK_ASSET_PATH = originalRoot;
        }
      },
    );
  }
