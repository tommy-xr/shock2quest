/** Measure live one-handed guns, without changing gameplay tuning.
 * npm run build && node dist/scripts/recoil-matrix.js /absolute/output/path
 * Defaults to the pistol's STR × Standard Weapons × AGI matrix.
 * --weapons=pistol,ar15,shotgun,laser,emp --skills=6 compares the gun families.
 * Add --mission=debug_weapons for unobstructed travel; the bench starts at
 * skill 6 and uses temporary stat modifiers to establish effective STR/AGI.
 * --strengths=1,3,6 --agilities=1,3,6 --repeats=3 narrow or repeat measurements.
 */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { parseArgs } from "node:util";
import { GameServer, attachSupportHand, type Vec3, type Quat } from "../src/index.js";
import { aimVrHandAt, sub } from "../test/helpers/vr-hand.js";
import { ammoOf, cycleToWeapon, muzzleFrameOf, moveSupportPump } from "../test/helpers/weapon.js";

const { values, positionals } = parseArgs({ allowPositionals: true, options: {
  weapons: { type: "string", default: "pistol" },
  strengths: { type: "string", default: "1,3,6" },
  skills: { type: "string", default: "1,3,6" },
  agilities: { type: "string", default: "1,3,6" },
  repeats: { type: "string", default: "3" },
  mission: { type: "string", default: "medsci1.mis" },
} });
const output = resolve(positionals[0] ?? "/tmp/pistol-recoil-matrix");
const weapons = { pistol: [-17, 1], ar15: [-18, 1], shotgun: [-19, 1], "shotgun-triple": [-19, 3], laser: [-22, 3], emp: [-23, 2] } as const;
const repeats = Number(values.repeats);
assert.ok(Number.isInteger(repeats) && repeats > 0);
const levels = (value: string) => {
  const parsed = value.split(",").map(Number).sort((a, b) => a - b);
  assert.ok(parsed.every(n => Number.isInteger(n) && n >= 1 && n <= 6));
  return [...new Set(parsed)];
};
await mkdir(output, { recursive: true });
const shots = [];
const metadata = [];
for (const weapon of values.weapons.split(",") as (keyof typeof weapons)[]) {
  assert.ok(weapon in weapons, `Unknown weapon: ${weapon}`);
  const [template, ammoUsage] = weapons[weapon];
  for (const strength of levels(values.strengths)) {
    for (const standard of levels(values.skills)) {
      await using game = await GameServer.launch({
        mission: values.mission,
        debugFlags: ["--vr"],
        experimental: ["physical_held_items"],
      });
      await game.step({ frames: 10 });
      const bench = values.mission === "debug_weapons";
      if (bench) {
        assert.equal(standard, 6, "the bench starts with weapon skills 6");
        await game.player.applyStatModifier({ source: "recoil-strength", stat: "strength", delta: strength - 6, duration_secs: 3600 });
      } else {
        await game.player.setStats({ strength, agility: 1, skills: { standard_weapons: standard, energy_weapons: 6 } });
      }
      const gun = await cycleToWeapon(game, e => e.template_id === template, { settleFrames: 90 });
      await aimVrHandAt(game, gun.position!, 0.45, 1);
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
      await game.input.set("left_hand.squeeze", 0);
      await game.input.set("head.rotation", [0, 0, 0, 1]);
      // Keep the offhand's support/pump target away from the belt MFD; a grip
      // over that UI is correctly consumed before it reaches gun handling.
      await game.input.set("right_hand.position", [0, 1, -1]);
      await game.input.set("right_hand.rotation", [0, Math.SQRT1_2, 0, Math.SQRT1_2]);
      await game.step({ frames: 300 });
      if (weapon === "shotgun-triple") {
        await game.input.trigger("CycleGunSetting");
        await game.step({ frames: 2 });
      }
      metadata.push({
        weapon, strength, standard, hz: 60, repeats, mission: values.mission, hand: "right", supported: false,
        runtime: await game.info(), gun: await game.entities.detail(gun.id),
        devParams: await game.devParams.list(),
      });
      await writeFile(join(output, "metadata.json"), JSON.stringify(metadata, null, 2));
      for (const agility of levels(values.agilities)) {
        if (bench) {
          await game.player.applyStatModifier({ source: "recoil-agility", stat: "agility", delta: agility - 6, duration_secs: 3600 });
        } else {
          await game.player.setStats({ strength, agility, skills: { standard_weapons: standard } });
        }
        for (let repeat = 0; repeat < repeats; repeat++) {
          if (ammoOf(await game.entities.detail(gun.id)) < ammoUsage) {
            assert.ok(template !== -22 && template !== -23, "Energy sample count exceeds the initial charge");
            for (const clip of template === -19 ? [-42, -43] : [-31, -32, -307]) await game.player.spawnItem(clip);
            await game.input.trigger("Reload");
            await game.step({ frames: 180 });
          }
          if (template === -19) {
            const pump = (await game.entities.detail(gun.id)).properties.find(p => p.name === "ShotgunPump");
            if (pump && JSON.parse(pump.value).phase !== "Ready") {
              await attachSupportHand(game, "right");
              await moveSupportPump(game, "right", 1);
              await moveSupportPump(game, "right", 0);
              await game.input.set("left_hand.squeeze", 0);
              await game.step({ frames: 2 });
            }
          }
          await game.step({ frames: 300 });
          const player = (await game.info()).player;
          assert.equal(player.hand_grips.find(g => g.hand === "right")?.support?.attached ?? false, false);
          assert.equal(player.effective_stats?.strength, strength);
          assert.equal(player.effective_stats?.agility, agility);
          assert.equal(player.stats?.skills.standard_weapons, standard);
          const initialDetail = await game.entities.detail(gun.id);
          const initial = muzzleFrameOf(initialDetail);
          const initialBody = (await game.physics.bodies({ entityId: gun.id })).bodies[0]!;
          const ammo = ammoOf(initialDetail);
          assert.ok(ammo > 0);
          const samples: { frame: number; ms: number; pitch: number; yaw: number; back: number; muzzle: ReturnType<typeof muzzleFrameOf>; body: { position: Vec3; rotation: Quat } }[] = [];
          await game.input.set("right_hand.trigger", 1);
          let previousFrame = 0;
          const frames = [...Array.from({ length: 30 }, (_, i) => i + 1), ...Array.from({ length: 25 }, (_, i) => 36 + i * 6)];
          for (const frame of frames) {
            await game.step({ frames: frame - previousFrame });
            previousFrame = frame;
            if (frame === 1) await game.input.set("right_hand.trigger", 0);
            const muzzle = muzzleFrameOf(await game.entities.detail(gun.id));
            const body = (await game.physics.bodies({ entityId: gun.id })).bodies[0]!;
            const delta = sub(body.position, initialBody.position);
            // World-space muzzle elevation and azimuth relative to the rested shot.
            const elevation = (v: number[]) => Math.atan2(v[1], Math.hypot(v[0], v[2]));
            const pitch = (elevation(muzzle.forward) - elevation(initial.forward)) * 180 / Math.PI;
            const yaw = Math.atan2(
              muzzle.forward[0] * initial.forward[2] - muzzle.forward[2] * initial.forward[0],
              muzzle.forward[0] * initial.forward[0] + muzzle.forward[2] * initial.forward[2],
            ) * 180 / Math.PI;
            const back = -delta.reduce((sum, value, axis) => sum + value * initial.forward[axis], 0);
            samples.push({ frame, ms: frame * 1000 / 60, pitch, yaw, back, muzzle, body: { position: body.position, rotation: body.rotation } });
          }
          assert.equal(ammoOf(await game.entities.detail(gun.id)), ammo - ammoUsage, "exactly one successful shot");
          assert.deepEqual((await game.info()).player.camera_rotation, player.camera_rotation);
          const peak = (axis: "back" | "pitch" | "yaw") => {
            const sample = samples.reduce((a, b) => Math.abs(b[axis]) > Math.abs(a[axis]) ? b : a);
            const magnitude = Math.abs(sample[axis]);
            const lastAbove = samples.findLast(s => Math.abs(s[axis]) > magnitude * 0.05);
            const recovery = lastAbove && samples.find(s => s.frame > lastAbove.frame);
            return { magnitude, signed: sample[axis], peakMs: magnitude < 1e-5 ? null : sample.ms, recovery95Ms: magnitude < 1e-5 ? null : recovery?.ms ?? null };
          };
          const result = { weapon, strength, standard, agility, repeat, stats: player.effective_stats, baseStats: player.stats, initial, initialBody, peaks: { back: peak("back"), pitch: peak("pitch"), yaw: peak("yaw") }, samples };
          shots.push(result);
          await writeFile(join(output, "measurements.json"), JSON.stringify(shots));
          console.log(JSON.stringify({ weapon, strength, standard, agility, repeat, peaks: result.peaks }));
        }
      }
    }
  }
}
console.log(`Saved ${shots.length} shots to ${output}`);
