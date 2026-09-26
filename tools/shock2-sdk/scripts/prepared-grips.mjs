// A manual pose is authoring data. Bulk baking may refresh automatic entries,
// but cannot erase overrides when geometry, hints, or the fitter change.
import assert from 'node:assert/strict';
export function preserveAuthoredGrips(previous, fresh) {
  assert.equal(previous.version, fresh.version, "Unsupported grip resource version");
  const entries = [...fresh.entries];
  for (const entry of previous.entries) {
    const index = entries.findIndex(e => e.model === entry.model && e.hand === entry.hand);
    const replacement = entries[index];
    if (!replacement) {
      // Additional models keep their exact prepared poses too.
      entries.push(entry);
      continue;
    }
    if (!entry.authored) continue;
    entries[index] = entry;
  }
  return {...fresh, entries};
}
