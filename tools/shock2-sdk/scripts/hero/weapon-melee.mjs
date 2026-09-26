// Run after `npm run build` in tools/shock2-sdk. Uses real VR grip inputs.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { GameServer } from '../../dist/src/index.js';
import { aimVrHandAt, quatFromTo, quatMultiply, quatRotate } from '../../dist/test/helpers/vr-hand.js';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
const { values } = parseArgs({ options: { output: { type: 'string', default: resolve(repoRoot, 'screenshots/hero') } } });
const output = resolve(values.output);
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

} finally {
  await game.shutdown();
}
