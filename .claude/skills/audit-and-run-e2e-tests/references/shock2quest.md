# shock2quest audit notes

Read the checkout's AGENTS.md, DEVELOPMENT.md, SDK package scripts and README; those remain authoritative. These notes cover pitfalls demonstrated by the E2E audit, not permanent game rules.

## Build and run identity

The SDK lives in `tools/shock2-sdk`. Current scripts clean `dist`, preflight the 25th Anniversary installation, and serialize full E2Es. Confirm the current scripts rather than assuming a command from an old checkout still applies.

- Set `DARK_ASSET_PATH` explicitly to the intended installation. A legacy unpacked install can load missions yet produce misleading remaster fixture failures. Check required mod archives as well as the main KPF.
- With the build-once runner available, each suite pins its runtime; `SHOCK2_RUNTIME_BINARY` can select an already verified binary. A filename alone is weak identity: record revision and executable hash. Direct SDK launches outside that runner may still invoke Cargo.
- Audit raw process launchers as well as `GameServer`. The lifecycle E2Es launch directly to test startup and idle behavior; those launches must honor the same pinned binary. A failing Cargo sentinel on `PATH` can verify that a prebuilt run does not silently fall back to rebuilding.
- Use scoped Rust checks (`shock2vr`, `desktop_runtime`, `debug_runtime`) when relevant. A shared target can be used by unrelated worktrees; avoid broad cache deletion or overwriting another session's executable. Require changed dependencies to rebuild and pin the completed artifact before another checkout can replace it.
- Source tests are TypeScript and generated tests are under `dist/test`. Clean before building and require a successful final compiler exit. Do not compile the snapshot that a long baseline/verification run is still reading.
- Serial execution is deliberate on memory-constrained machines. Parallel suites need private mutable save/settings state and compatible port usage, not merely more workers. The SDK's ephemeral ports and runtime instance IDs help ownership checks.

## Fixture diagnosis

- Discover runtime entities by template/name. Runtime IDs are not stable between launches. Exposed IDs can also be reused after a transient entity expires within a run; a before/after set difference can discard a new bullet impact. Prefer monotonic message/audio event sequences when measuring events, and verify their physical positions and semantic tags.
- `/v1/step` advances fixed 60 Hz simulation time and blocks until complete; HTTP sleeps do not advance particles or timers. Async AI path work can vary slightly, so use behavioral bounds rather than exact-frame positions.
- Developer-menu tests must provision the `developer-mode` sentinel, preferably through `launchDeveloperGame`; launching a debug scene alone does not enable the Developer entry. Keep the sentinel and settings private instead of relying on the user's installation.
- Check health and actual player placement before diagnosing missing actions. Eye-level pickup staging can put the body below a floor; a longer scenario may die after the pickup succeeds. Invulnerable observers still need meaningful placement when testing AI.
- For AI convergence tests, verify complete walk routes from the intended pursuers to the settled observer. A nearby point can be an unreachable recess, and valid detours can initially increase straight-line distance. Move the observer out of sight before a separate alertness-decay control; arrival legitimately makes it visible.
- Check weapon skill, research, upgrades, and inventory capacity. Read current production contracts: biological alternate modes may be innate; manufactured modes can require installation. Do not assume an earlier test's requirements remain valid.
- Raw controller input, calibrated glove origins, palm contacts, and model grip anchors are distinct. Shared hand helpers should invert calibration exactly once. Some runtime diagnostics already return raw controller coordinates; check their contract before applying another offset.
- Body gear can consume a squeeze intended for a support grip. Stage the weapon away from pouch/holster regions when the test targets support mechanics. Retained melee support can deliberately transfer a weapon when the primary hand releases.
- Use actual ladder rungs/rails from `physics.ladder`, not points on an obsolete solid-box face. Snap turns require fresh stick edges; holding the stick is not repeated turning.
- Prefer render provenance to undifferentiated draw counts. Replacement particles, procedural gloves, watch bands and weapon meshes can share an entity or broad source label. Verify which object or panel the test actually measured.
- Pre-v1 save compatibility is not a requirement. Do not add migration code just to load an old private regression fixture. Same-build round trips still must work.

## Optional coverage

Check each test's actual gate. The audit encountered private-save gates for SHODAN boss entry/interlocks/ending, Ops2 corpse blockage, Rec/Command arrival, Rick1 card corpse/Rumbler junction, Rick2 mixed-case loading, and an exact rotating-door approach. Report each unavailable scenario explicitly.

`SHOCK2_ACCURACY_FIXTURE` points to a private asset tree with nonzero SKILLPARAM accuracy data. The existing test describes the exact byte change. A local fixture can be derived from the licensed configured installation without modifying or publishing shipped assets; verify its version and run both Strength cases. Keep its mutable saves separate.

For actual visible product changes, follow the repository's pr-visuals skill and flat/VR verification requirements. Updating an obsolete test fixture or debug-only metadata does not by itself require changing game visuals.

## Follow-up failure diagnosis

- Reuse a same-build save from the failing run when the divergence occurs after load. Copy it into a separate diagnostic asset root, replay only the failing stage, and preserve the full run’s mutable data. A rare Overlord aiming failure reproduced immediately from its retained mortal-state save after five fresh scenarios passed. Do not publish licensed saves or asset trees with the audit report.
- Haptic sequence counters include every request source. A charge-only test beside a live pipe hybrid can count a legitimate parry pulse; inspect `PipeAttack.parries`, impact audio, and player health before changing pulse expectations. Stage isolated feedback checks away from combat while keeping exact assertions.
- Check disk capacity alongside RAM. Old per-worktree Rust targets can occupy tens of GiB even when the audit’s own data is small. Before reclaiming a target cache, verify its exact path and cache marker, inspect worktree status, and check that no active process uses it. Preserve source checkouts, game data, saves, and evidence; do not clear an active shared Cargo target. Record any interrupted runner’s real exit and resumed file coverage.
- For a fixed-controller or recoil check, record the pawn pose as well as input. A stationary local controller moves in world space when its pawn moves. Use an appropriate clear fixture, assert the stationary prerequisite, and project motion onto the actual weapon axis instead of assuming a scene’s spawn yaw. Preserve the original recovery thresholds.
