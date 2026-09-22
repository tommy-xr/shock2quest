import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateWorkload, runOrder } from './run.mjs';

const fixture = { name: 'stress', object_lighting: true, expected_subject_meshes: 2, spawns: [{}] };
const sample = frame => ({ name: 'stress', setup_complete: true, object_lighting: true, subject_meshes: 2,
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
