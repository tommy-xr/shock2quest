// A manual pose is authoring data. Bulk baking may refresh automatic entries,
// but cannot erase an override or carry it across changed geometry/rig/hints.
import assert from 'node:assert/strict';
export function preserveAuthoredGrips(previous, fresh) {
  const entries = [...fresh.entries];
  for (const entry of previous.entries.filter(e => e.authored)) {
    const index = entries.findIndex(e => e.model === entry.model && e.hand === entry.hand);
    const replacement = entries[index];
    assert.ok(replacement && previous.version === fresh.version && previous.solver_revision === fresh.solver_revision &&
      ['surface_hash', 'kinematics_hash', 'hints_hash'].every(key => entry[key] === replacement[key]),
      `Manual override ${entry.model} ${entry.hand} needs review in Explorer before rebaking; no output written`);
    entries[index] = entry;
  }
  return {...fresh, entries};
}
