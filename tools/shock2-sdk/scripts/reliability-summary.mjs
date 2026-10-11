// Ignore the aggregate summary: Node may count an empty test file as a pass.
export default async function* summaries(events) {
  for await (const event of events) {
    if (event.type === "test:summary" && event.data.file) {
      yield JSON.stringify(event.data) + "\n";
    }
  }
}
