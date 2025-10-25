# Speech Step 6 – Tooling & Debug Support

## Objective
Equip developers with diagnostics and automated checks to verify speech selection, aiding future maintenance.

## Prerequisites
- Core speech system functional (Steps 1–5).
- Comfort with existing logging/tracing infrastructure (`tracing`, CLI tools).
- Optional: knowledge of `tools/dark_query`.

## Deliverables
- Debug logging utilities or CLI commands to inspect speech queries and selections.
- Tests/fixtures validating tag conversion and selection outputs.
- Documentation (README snippet) explaining how to run diagnostics.

## Detailed Tasks
1. **Runtime Logging**
   - Add `tracing` instrumentation around speech selection (input tags, voice, selected schema/sample, weight).
   - Provide feature flag or configuration to enable verbose logging without spamming production builds.
   - Ensure sensitive data (player position) not overly exposed.

2. **Developer Command / Tool**
   - Extend `tools/dark_query` or create new CLI to query voice by template, concept, and optional tags and print sample results (using offline data).
   - Optionally add console command in runtime (if available) to dump current voice registry or active speech state.

3. **Automated Tests**
   - Add tests verifying tag builder outputs (Step 3) for known scenarios.
   - Add snapshot/expected result tests for `select_speech_schema` using recorded tag sets (possibly under `tests/`).
   - Ensure tests run in CI (`cargo test --workspace`).

4. **Documentation**
   - Update appropriate README or `projects/speech.md` with instructions for enabling logging and running tools.
   - Include troubleshooting tips (e.g., missing voice data, empty tag matches).

## Open Questions / Research
- Does existing debug infrastructure allow toggling tracing categories at runtime? Investigate current logging configuration.
- Determine best place to hook CLI command (Rust binary vs existing script).

## Acceptance Criteria
- Developers can enable a debug mode that prints voice/concept/tag/sample info for each speech event.
- CLI/tool command successfully queries at least one concept and prints weighted results.
- New tests cover key conversion and selection logic without flaking.

## Handoff Checklist
- Provide example command usage and expected output.
- Ensure documentation references log filters or environment variables needed.
