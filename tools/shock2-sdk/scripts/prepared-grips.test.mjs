import test from 'node:test';
import assert from 'node:assert/strict';
import {preserveAuthoredGrips} from './prepared-grips.mjs';
const entry = {model:'mug', hand:'left',surface_hash:'mesh',kinematics_hash:'rig',hints_hash:'hints',grip:{offset:1}};
const library = entries => ({version:1,solver_revision:2,entries});
test('bulk baking preserves authored hands and updates automatic hands', () => {
  const manual = {...entry,authored:true};
  const right = {...entry,hand:'right'};
  const fresh = library([{...entry,grip:{offset:2}}, {...right,grip:{offset:3}}]);
  const result = preserveAuthoredGrips(library([manual,right]),fresh);
  assert.deepEqual(result.entries,[manual,fresh.entries[1]]);
  assert.equal(fresh.entries[0].grip.offset,2);
});
test('changed inputs and fitter revisions preserve model overrides', () => {
  const previous=library([{...entry,authored:true}]);
  for (const key of ['surface_hash','kinematics_hash','hints_hash']) {
    assert.deepEqual(preserveAuthoredGrips(previous,library([{...entry,[key]:'changed'}])).entries,previous.entries);
  }
  assert.deepEqual(preserveAuthoredGrips(previous,library([])).entries,previous.entries);
  assert.deepEqual(preserveAuthoredGrips(library([entry]),library([])).entries,[entry]);
  assert.deepEqual(preserveAuthoredGrips(previous,{...library([entry]),solver_revision:3}).entries,previous.entries);
});
