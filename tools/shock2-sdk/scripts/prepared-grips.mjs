// A manual pose is authoring data. Bulk baking may refresh automatic entries,
// but cannot erase an override or carry it across changed geometry/rig/hints.
import assert from 'node:assert/strict';
export function preserveAuthoredGrips(previous, fresh) {
  const entries = [...fresh.entries];
  for (const entry of previous.entries) {
    const index = entries.findIndex(e => e.model === entry.model && e.hand === entry.hand);
    const replacement = entries[index];
    if (!replacement) {
      assert.ok(previous.version === fresh.version && previous.solver_revision === fresh.solver_revision,
        `Additional model ${entry.model} ${entry.hand} needs review after a solver revision change`);
      // Explorer can add models that have no rack fixture. Keep their exact
      // entries; runtime fingerprint checks still reject stale geometry/rig/hints.
      entries.push(entry);
      continue;
    }
    if (!entry.authored) continue;
    assert.ok(previous.version === fresh.version && previous.solver_revision === fresh.solver_revision &&
      ['surface_hash', 'kinematics_hash', 'hints_hash'].every(key => entry[key] === replacement[key]),
      `Manual override ${entry.model} ${entry.hand} needs review in Explorer before rebaking; no output written`);
    entries[index] = entry;
  }
  return {...fresh, entries};
}
