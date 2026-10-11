import { resolve } from "node:path";

// A filtered-out file can itself pass without running any tests. Its empty
// file plan precedes that synthetic pass. Use events supported by Node 20 too.
export default async function* summaries(events) {
  const emptyFiles = new Set();
  let passed = 0;
  for await (const { type, data } of events) {
    if (type === "test:plan" && data.file && data.nesting === 0 && data.count === 0) {
      emptyFiles.add(data.file);
    }
    if (type === "test:pass" && !data.skip && !data.todo && data.details.type !== "suite" &&
        !(emptyFiles.has(data.file) && resolve(data.name) === resolve(data.file))) {
      passed++;
    }
  }
  yield JSON.stringify({ counts: { passed } }) + "\n";
}
