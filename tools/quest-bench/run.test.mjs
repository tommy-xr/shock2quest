import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateWorkload, runOrder } from './run.mjs';

const fixture = { name: 'stress', object_lighting: true, expected_subject_meshes: 2, spawns: [{}] };
const sample = frame => ({ name: 'stress', setup_complete: true, object_lighting: true, ffr_requested: 'off', ffr_effective: 'off', subject_meshes: 2,
  lit_subject_meshes: 2, lamp_intensities: [], animations: [{ entity: 12, clip: 'idle', frame }] });
const logs = samples => samples.map(s => `SHOCK2QUEST_PERF focused=true\nSHOCK2QUEST_BENCHMARK ${JSON.stringify(s)}`).join('\n');

test('validates the measured tail and accepts moving animation', () => {
  assert.equal(validateWorkload(logs([{}, sample(1), sample(2)]), fixture, 2).length, 2);
});
test('rejects missing, unlit, culled and frozen workloads', () => {
  for (const samples of [[sample(1)], [sample(1), sample(1)],
    [sample(1), { ...sample(2), subject_meshes: 0 }],
    [sample(1), { ...sample(2), lit_subject_meshes: 0 }],
    [sample(1), { ...sample(2), lamp_intensities: [null] }],
    [sample(1), { ...sample(2), setup_complete: false }],
    [sample(1), { ...sample(2), animations: [] }]]) {
    assert.throws(() => validateWorkload(logs(samples), fixture, 2));
  }
});
test('balances paired runs as ABBA', () => {
  assert.deepEqual(runOrder(2, 'both').map(run => run.mode), ['off', 'on', 'on', 'off']);
  assert.deepEqual(runOrder(2, 'on').map(run => run.mode), ['on', 'on']);
});
test('a missing workload cannot be replaced by buffered warmup evidence', () => {
  assert.throws(() => validateWorkload(logs([sample(1), sample(2)]) + '\nSHOCK2QUEST_PERF focused=true', fixture, 2));
});
test('rejects animation that advances once and then freezes', () => {
  assert.throws(() => validateWorkload(logs([sample(1), ...Array(29).fill(sample(2))]), fixture, 30));
  assert.equal(validateWorkload(logs(Array.from({ length: 30 }, (_, i) => sample(i % 3))), fixture, 30).length, 30);
});
test('requires every authored lamp to match the fixture state', () => {
  for (const lights_on of [false, true]) {
    const litFixture = { ...fixture, lights_on, light_templates: [711, 714] };
    const samples = [sample(1), sample(2)].map(s => ({ ...s, lamp_intensities: Array(2).fill(lights_on ? 1 : 0) }));
    assert.equal(validateWorkload(logs(samples), litFixture, 2).length, 2);
    samples[1].lamp_intensities[0] = lights_on ? 0 : 1;
    assert.throws(() => validateWorkload(logs(samples), litFixture, 2));
  }
});

test('balances three terrain modes independently of object lighting', async () => {
  const { terrainRuns, terrainFixture } = await import('./run.mjs');
  assert.deepEqual(terrainRuns(2, 'on', 'all').map(run => run.terrain),
    ['classic', 'upgraded', 'wet', 'wet', 'upgraded', 'classic']);
  assert.equal(terrainFixture({}, 'classic').upgraded_terrain, false);
  assert.equal(terrainFixture({}, 'upgraded').terrain_wetness, 0);
  assert.equal(terrainFixture({}, 'wet').terrain_wetness, 1.5);
});

test('mixed crowds require all model groups but static eggs need no animation', () => {
  const mixed = { ...fixture, upgraded_terrain: true, terrain_wetness: 1.5,
    additional_subjects: [{ model: 'eggcl', expected_meshes: 4 }], spawns: [{}, { animated: false }] };
  const samples = [sample(1), sample(2)].map(s => ({ ...s, upgraded_terrain: true, terrain_wetness: 1.5,
    additional_subjects: [{ model: 'eggcl', meshes: 4, lit_meshes: 4 }] }));
  assert.equal(validateWorkload(logs(samples), mixed, 2).length, 2);
  for (const patch of [{ upgraded_terrain: false }, { terrain_wetness: 0 }, { additional_subjects: [] },
    { additional_subjects: [{ model: 'eggcl', meshes: 3, lit_meshes: 3 }] }]) {
    assert.throws(() => validateWorkload(logs([samples[0], { ...samples[1], ...patch }]), mixed, 2));
  }
});

test('GPU output uses metric names and accepts terminal line endings', async () => {
  const { parseGpuCounters } = await import('./run.mjs');
  const gpu = parseGpuCounters('GPU % Utilization : 40.0\r\n\r\nGPU % Utilization : 60.0\r\nnoise');
  assert.equal(gpu['GPU % Utilization'].mean, 50);
  assert.deepEqual(parseGpuCounters('no permission'), {});
});

test('rejects unavailable GPU counter sentinels instead of averaging them', async () => {
  const { parseGpuCounters } = await import('./run.mjs');
  assert.throws(() => parseGpuCounters('Fragments Shaded / Second : -1.000'), /invalid GPU counter/);
});


test('FFR matrix balances positions and ordered neighbors across four repeats', async () => {
  const { ffrRuns } = await import('./run.mjs');
  const runs = ffrRuns(4, 'on', 'upgraded', 'all');
  assert.equal(runs.length, 16);
  for (let position = 0; position < 4; position++) {
    assert.equal(new Set(runs.filter((_, i) => i % 4 === position).map(run => run.ffr)).size, 4);
  }
  const pairs = [];
  for (let i = 0; i < runs.length; i++) if (i % 4 !== 3) pairs.push(`${runs[i].ffr}/${runs[i + 1].ffr}`);
  assert.equal(new Set(pairs).size, 12);
});

test('FFR rejects unsupported fallback, missing evidence and dynamic runtime overrides', async () => {
  const { validateFoveation } = await import('./run.mjs');
  const high = { ...fixture, ffr: 'high' };
  const samples = [sample(1), sample(2)].map(s => ({ ...s, ffr_requested: 'high', ffr_effective: 'high' }));
  assert.equal(validateWorkload(logs(samples), high, 2).length, 2);
  for (const ffr_effective of ['off', undefined]) {
    assert.throws(() => validateWorkload(logs([samples[0], { ...samples[1], ffr_effective }]), high, 2), /FFR/);
  }
  validateFoveation({ foveation_level: { min: 3, max: 3 }, dynamic_foveation_samples: 0 }, 'high');
  for (const value of [undefined, { foveation_level: { min: 0, max: 3 }, dynamic_foveation_samples: 0 },
    { foveation_level: { min: 3, max: 3 }, dynamic_foveation_samples: 1 }]) {
    assert.throws(() => validateFoveation(value, 'high'), /FFR/);
  }
});


test('CPU diagnostics require matching windows and complete frame histogram counts', async () => {
  const { validateCpuProfile } = await import('./run.mjs');
  const sample = { cpu_frames: 90, phases: { game_update: { calls: 90 } }, cpu_histogram_100us: [[80, 90]] };
  const log = `SHOCK2QUEST_CPU_PROFILE ${JSON.stringify(sample)}\nSHOCK2QUEST_PERF focused=true\n`;
  assert.equal(validateCpuProfile(log + log, 2).length, 2);
  assert.throws(() => validateCpuProfile(log + 'SHOCK2QUEST_PERF focused=true\n', 2), /diagnostics/);
  sample.cpu_histogram_100us[0][1] = 89;
  assert.throws(() => validateCpuProfile(`SHOCK2QUEST_CPU_PROFILE ${JSON.stringify(sample)}\nSHOCK2QUEST_PERF focused=true`, 1), /diagnostics/);
});
