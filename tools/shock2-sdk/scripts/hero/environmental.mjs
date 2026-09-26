#!/usr/bin/env node
// Run after `npm run build` in tools/shock2-sdk. Images are actual VR-runtime
// renders, with a detached camera for unobstructed environmental framing.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { GameServer } from '../../dist/src/index.js';
import { switchCourtLights } from '../../dist/test/helpers/rec1-lights.js';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
const { values } = parseArgs({ options: { output: { type: 'string', default: resolve(repoRoot, 'screenshots/hero') } } });
const output = resolve(values.output);
const recipes = [
  {
    name: 'hydroponics', mission: 'hydro1.mis',
    caption: 'Hydroponics plant corridor and maintenance robot',
    position: [19, 2.4, 38], lookAt: [19, 1.5, 22], frames: 1,
    lighting: 'Unmodified mission lighting',
  },
  {
    name: 'recreation-pool', mission: 'rec1.mis',
    caption: 'Recreation deck swimming pool',
    position: [24, -3, -236], lookAt: [3, -4, -226], frames: 120,
    lighting: 'Authored court lamps switched on through their TurnOn messages; no lighting overrides',
  },
];
await mkdir(output, { recursive: true });
const revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim();
for (const recipe of recipes) {
  const game = await GameServer.launch({ mission: recipe.mission, repoRoot, debugFlags: ['--vr'] });
  try {
    await game.step({ frames: 1 });
    await game.devParams.set('free_camera_cull', 1);
    if (recipe.name === 'recreation-pool') await switchCourtLights(game, 'TurnOn');
    await game.camera.set({ position: recipe.position, lookAt: recipe.lookAt });
    await game.step({ frames: recipe.frames });
    const capture = await game.screenshot(resolve(output, `${recipe.name}.png`), 1600);
    assert.ok(capture.size_bytes > 10000, 'Screenshot must contain rendered image data');
    await writeFile(resolve(output, `${recipe.name}.json`), `${JSON.stringify({
      ...recipe, revision, capturedAt: new Date().toISOString(), presentation: 'vr',
      camera: 'detached', requestedMaxWidth: 1600, resolution: capture.resolution,
      timestepHz: 60, renderSettings: 'Defaults except free_camera_cull=1',
    }, null, 2)}\n`);
    console.log(`${recipe.name}: ${capture.full_path} (${capture.resolution.join('×')})`);
  } finally {
    await game.shutdown();
  }
}
