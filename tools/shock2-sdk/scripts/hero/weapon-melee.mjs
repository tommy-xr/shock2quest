// Run after `npm run build` in tools/shock2-sdk. Uses real VR grip inputs.
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { tmpdir } from 'node:os';
import { GameServer, lookQuat } from '../../dist/src/index.js';
import { aimVrHandAt, quatConjugate, quatFromTo, quatMultiply, quatRotate } from '../../dist/test/helpers/vr-hand.js';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
const { values } = parseArgs({ options: { video: { type: 'boolean', default: false }, output: { type: 'string', default: resolve(repoRoot, 'screenshots/hero') } } });
const output = resolve(values.output);
if (values.video) execFileSync('ffmpeg', ['-version'], { stdio: 'ignore' });
await mkdir(output, { recursive: true });
const game = await GameServer.launch({ mission: 'medsci1.mis', debugFlags: ['--vr'], repoRoot });
const held = {};
try {
  await game.step({ frames: 1 });
  // DebugCycleWeapon provisions the normal weapon entities. VR still requires
  // physically grabbing them. Unused cycle items stay behind at the spawn.
  for (const [hand, template, cycles] of [['right', -17, 1], ['left', -928, 11]]) {
    const before = new Set((await game.entities.byTemplate(template)).map(e => e.id));
    for (let i = 0; i < cycles; i++) {
      await game.input.set('head.look', [0, 0]);
      await game.input.trigger('DebugCycleWeapon');
      await game.step({ frames: 1 });
    }
    await game.step({ frames: 90 });
    const item = (await game.entities.byTemplate(template)).find(e => !before.has(e.id));
    assert.ok(item, `Missing provisioned template ${template}`);
    await aimVrHandAt(game, item.position, 0.1, 1, 0, { hand });
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player[hand === 'left' ? 'wielded_entity_id' : 'right_hand_entity_id'], item.id);
    held[hand] = item.id;
    await game.input.set(`${hand}_hand.position`, [-0.55, 1.4, hand === 'left' ? 0.25 : -0.25]);
    await game.input.set(`${hand}_hand.rotation`, [0, Math.SQRT1_2, 0, Math.SQRT1_2]);
  }
  const position = { x: 16, y: 1, z: 17 };
  const rotation = quatFromTo([0, 0, -1], [-1, 0, 0]);
  await game.player.teleport(position);
  await game.input.set('head.rotation', rotation);
  const hands = {};
  for (const hand of ['left', 'right']) {
    const pos = quatRotate(rotation, [hand === 'left' ? -0.18 : 0.18, 0, -0.85]);
    pos[1] = hand === 'left' ? 0.75 : 0.83;
    const rot = hand === 'left' ? quatMultiply(rotation, [Math.sin(-0.22), 0, 0, Math.cos(0.22)]) : rotation;
    await game.input.set(`${hand}_hand.position`, pos);
    await game.input.set(`${hand}_hand.rotation`, rot);
    hands[hand] = { position: pos, rotation: rot };
  }
  // Let the physical wrench and player settle after teleporting.
  await game.step({ frames: 90 });
  const state = await game.info();
  assert.equal(state.player.life_state, 'alive');
  assert.equal(state.player.wielded_entity_id, held.left);
  assert.equal(state.player.right_hand_entity_id, held.right);
  assert.ok(Math.abs(state.player.position[1] + 0.356) < 0.1, 'Player must settle on the medical-room floor');
  const result = await game.screenshot(resolve(output, 'medsci-weapon-melee.png'), 1600);
  assert.ok(result.size_bytes > 10000, 'Screenshot must contain rendered image data');
  await writeFile(resolve(output, 'medsci-weapon-melee.json'), JSON.stringify({
    revision: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim(),
    mission: 'medsci1.mis', presentation: 'vr', kind: 'staged loadout still',
    position, settledPosition: state.player.position, rotation, hands,
    simulationFrame: state.frame_index, capturedAt: new Date().toISOString(), timestepHz: 60,
    screenshot: { file: 'medsci-weapon-melee.png', resolution: result.resolution, size_bytes: result.size_bytes },
    note: 'Actual VR pistol and wrench grips; debug-provisioned loadout, authored lighting. Not evidence of a combat encounter.',
  }, null, 2) + '\n');
  console.log(resolve(output, 'medsci-weapon-melee.png'));
  if (values.video) {
    // Each take gets a fresh sequence: stale frames from an older, longer take
    // must never be appended to the next export or clutter the repository.
    const frameDir = await mkdtemp(resolve(tmpdir(), 'shock2quest-hero-'));
    await game.devParams.set('fov_override_deg', 80);
    await game.player.setStats({ skills: { standard_weapons: 1 } });
    // Stage a fresh opponent in front of the still's player position. All damage below comes from trigger and tracked-hand inputs.
    const previousEnemies = new Set((await game.entities.byTemplate(-397)).map(e => e.id));
    await game.input.trigger('SpawnDebugMonster');
    await game.step({ frames: 1 });
    const enemy = (await game.entities.byTemplate(-397)).find(e => !previousEnemies.has(e.id));
    assert.ok(enemy, 'Expected a fresh debug-provisioned pipe hybrid');
    await game.player.teleport({ x: 16, y: state.player.position[1], z: 17 });
    await game.step({ frames: 1 });
    const hp = async () => Number((await game.entities.detail(enemy.id)).properties.find(p => p.name === 'HitPoints')?.value);
    const initialHp = await hp();
    const ammo = async () => Number((await game.entities.detail(held.right)).properties.find(p => p.name === 'Ammo')?.value);
    const initialAmmo = await ammo();
    let captured = 0;
    const capture = async () => {
      await game.screenshot(resolve(frameDir, `frame-${String(captured++).padStart(4, '0')}.png`), 800);
    };
    const advance = async frames => {
      for (let f = 0; f < frames; f += 3) {
        await game.step({ frames: Math.min(3, frames - f) });
        await capture();
      }
    };
    await advance(3);
    const target = await game.player.aimAt(enemy.id, { hitbox: 'torso', visibility: 'required' });
    const pawn = (await game.info()).player;
    const handWorld = pawn.position.map((v, i) => v + hands.right.position[i]);
    await game.input.set('head.rotation', rotation);
    await game.input.set('right_hand.rotation', quatFromTo([0, 0, -1], target.world_point.map((v, i) => v - handWorld[i])));
    await game.input.set('right_hand.trigger', 1);
    await advance(3);
    await game.input.set('right_hand.trigger', 0);
    await advance(27);
    const secondAim = await game.player.aimAt(enemy.id, { hitbox: 'torso', visibility: 'required' });
    await game.input.set('head.rotation', rotation);
    await game.input.set('right_hand.rotation', quatFromTo([0, 0, -1], secondAim.world_point.map((v, i) => v - handWorld[i])));
    await game.input.set('right_hand.trigger', 1);
    await advance(3);
    await game.input.set('right_hand.trigger', 0);
    await advance(30);
    const gunHp = await hp();
    const afterAmmo = await ammo();
    assert.equal(afterAmmo, initialAmmo - 2, 'The pistol must expend two actual rounds');
    assert.ok(gunHp < initialHp && gunHp > 0, `Gun must damage but leave a melee target (${initialHp} -> ${gunHp})`);
    // Wait for melee range, retaining the continuous simulation in the clip.
    for (let n = 0; n < 40; n++) {
      const current = await game.entities.detail(enemy.id);
      const player = (await game.info()).player;
      if (Math.hypot(current.position[0] - player.position[0], current.position[2] - player.position[2]) < 2.8) break;
      await advance(3);
    }
    await game.input.set('left_hand.trigger', 1);
    for (let f = 0; f < 45; f++) {
      const current = await game.entities.detail(enemy.id);
      const torso = current.aim_points.find(p => p.classification === 'torso')?.position ?? current.position;
      const player = (await game.info()).player;
      const eye = player.position.map((v, i) => v + (i === 1 ? player.camera_offset[1] : 0));
      const delta = torso.map((v, i) => v - eye[i]);
      const length = Math.hypot(...delta);
      const direction = delta.map(v => v / length);
      const worldRotation = lookQuat(direction);
      // Wrench head sits above the fist; sweep its contact through the torso.
      // This follows the existing medsci-saved-vr-melee scenario gesture.
      const contactOffset = quatRotate(worldRotation, [0, 0.75, 0]);
      const distance = 1.5 - 2.25 * f / 44;
      const worldHand = torso.map((v, i) => v - direction[i] * distance - contactOffset[i]);
      await game.input.set('left_hand.position', quatRotate(quatConjugate(player.rotation), worldHand.map((v, i) => v - player.position[i])));
      await game.input.set('left_hand.rotation', quatMultiply(quatConjugate(player.rotation), worldRotation));
      await game.step({ frames: 1 });
      if (f % 3 === 2) await capture();
    }
    await game.input.set('left_hand.trigger', 0);
    const meleeHp = await hp();
    assert.equal(meleeHp, 0, `Tracked wrench swing must finish the target (${gunHp} -> ${meleeHp})`);
    for (const hand of ['left', 'right']) {
      await game.input.set(`${hand}_hand.position`, hands[hand].position);
      await game.input.set(`${hand}_hand.rotation`, hands[hand].rotation);
    }
    await advance(120);
    const finalPlayer = (await game.info()).player;
    assert.equal(finalPlayer.life_state, 'alive');
    assert.equal(finalPlayer.wielded_entity_id, held.left);
    assert.equal(finalPlayer.right_hand_entity_id, held.right);
    await writeFile(resolve(output, 'combat-evidence.json'), JSON.stringify({
      kind: 'Staged debug opponent; normal VR inputs cause gun and wrench damage',
      revision: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim(),
      mission: 'medsci1.mis', presentation: 'vr', timestepHz: 60,
      capturedAt: new Date().toISOString(),
      template: -397, initialHp, afterGun: gunHp, afterMelee: meleeHp, initialAmmo, afterAmmo,
      standardWeaponsSkill: 1,
      frames: captured, fps: 20, fovOverrideDegrees: 80,
      note: 'Prototype motion; hard cut to opening frame when looping. No damage messages injected.',
    }, null, 2) + '\n');
    const inputPattern = resolve(frameDir, 'frame-%04d.png');
    execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-framerate', '20', '-i', inputPattern,
      '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', resolve(output, 'weapon-melee.mp4')]);
    execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-framerate', '20', '-i', inputPattern,
      '-filter_complex', 'fps=20,scale=640:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse',
      '-loop', '0', resolve(output, 'weapon-melee.gif')]);
    console.log(`Combat prototype: ${captured / 20}s; HP ${initialHp} -> ${gunHp} -> ${meleeHp}`);
    console.log(`Raw review frames: ${frameDir}`);
  }

} finally {
  await game.shutdown();
}
