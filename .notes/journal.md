## Compiler Warnings Cleanup Complete
- Fixed 9 compiler warnings by addressing unused variables, imports, struct fields, and methods across multiple packages (reduced total warnings from 229 to 220).
- Learning: The `cargo fix` tool helps with simple cases like unused imports, but for parsed data structures from game files, `#[allow(dead_code)]` attributes are more appropriate than removing fields that represent the binary format.

## Teleport System Integration Complete
- Integrated Phase 1 VR teleport system into main game loop by adding TeleportSystem to Game struct and wiring update calls to process input and generate SetPlayerPosition effects.
- Learning: The existing command_effects pattern in Game::update() provided the perfect integration point, and following the established Effect system architecture made the integration seamless and testable.

## Enhanced Lighting Hand Spotlights Complete
- Successfully addressed PR #81 feedback by implementing functional hand spotlights for testing the enhanced lighting system, replacing visual cube indicators with actual SpotLight objects that participate in multi-pass lighting.
- Learning: The architecture challenge of Mission trait returning Vec<SceneObject> vs Scene with lights was solved by adding a separate method for lights and integrating them at the runtime level after Scene creation, maintaining clean separation of concerns.

## Asset Validation Mission Complete
- Implemented Phase 2.2 of Mission System project with complete AssetValidationMission including VR UI framework, state machine, controller input, and comprehensive test coverage (5 passing tests).
- Learning: The Mission trait infrastructure from Phase 1 provided an excellent foundation, and creating test-friendly asset validation logic required separating file system operations from core validation logic for better testability.

## Multi-Pass Lighting Step 2 Complete
- Implemented Step 2 of multi-pass lighting system with 3-pass rendering (base → per-light additive → transparent), proper OpenGL state management, and light culling foundation.
- Learning: The existing two-pass rendering system in gl_engine.rs:125-137 was perfectly positioned for multi-pass lighting extension, and the Light trait's `affects_position()` method enables efficient culling at the object level.

## Performance Optimization Complete
- Implemented safe performance improvements including FxHashMap migration, Cargo resolver 2, AssetCache optimizations, and memory efficiency improvements.
- Learning: FxHashMap provides significant performance benefits for numeric keys (EntityId), and analyzing performance bottlenecks with `cargo tree --duplicates` revealed optimization opportunities in the build system.

## Clippy Code Style Warnings Fixed
- Fixed 29 clippy warnings by removing unneeded unit return types and redundant field names across 8 files, improving code clarity and Rust idioms compliance.
- Learning: The `cargo clippy` tool provides excellent actionable feedback for code style improvements, and fixing warnings systematically using MultiEdit tool made bulk changes efficient and error-free.

## PR Review Complete
- Reviewed all open PRs: #62 ready to auto-merge, #63 in progress, #31 needs major rework due to conflicts and age.
- Learning: Large, long-running PRs accumulate significant technical debt - prefer smaller, frequent PRs as per CLAUDE.md guidelines.

## Logging Phase 3 Migration Complete
- Systematically migrated script and mission system println! statements to structured, scoped logging using the improved logging infrastructure.

## Logging Phase 4 Save/Load Migration Complete

## Multi-Pass Lighting Step 3 Material System Complete
- Implemented Step 3 of multi-pass lighting with spotlight support for all material types (BasicMaterial, SkinnedMaterial, LightmapMaterial, BillboardMaterial) including proper shader compilation, uniform management, and performance optimizations.
- Learning: The dual shader approach (base + lighting shaders per material) provides clean separation of concerns, and enhancing the Light trait with spotlight_params() eliminated the need for unsafe trait casting in shader uniform setup.
- Continued systematic migration by updating save/load system and FFmpeg video decoder to use scoped logging, migrating 3 remaining println! statements to proper structured logging.
- Learning: Macro exports are at crate root (engine::game_log) not submodules (engine::logging::game_log), and systematic migration is most effective when targeting related subsystems together rather than scattered files.
- Learning: Match arm expressions can't contain statement-generating macros directly - wrap in braces for proper syntax; macro design needs to consider usage context for proper expansion.

## Improved Logging Project Complete
- Completed final migration of all active println! statements across entity system, speech database, physics, debugging utilities, and Android platform code to structured/scoped logging.
- Learning: Thorough `rg "println!" --type rust` search and systematic file-by-file migration is more effective than trying to find patterns - ensures comprehensive coverage of scattered debug prints.

## Entity Inspector CLI Tool Phase 1 Complete
- Implemented comprehensive CLI tool for inspecting System Shock 2 entity data with support for gamesys/mission files, export formats (JSON/CSV), template queries, and basic validation.
- Learning: The `dark::properties::get()` function returns a tuple `(properties, links, links_with_data)` not individual getter functions, and mission file parsing requires both AssetCache and Gamesys parameters for proper entity merging.

## VR Teleport System Phase 1 Complete
- Implemented foundational teleport system infrastructure with configurable input detection, per-hand state tracking, and integration with existing effect system.
- Learning: Rust borrow checker requires careful API design - static methods with explicit config parameters avoid borrowing conflicts when updating multiple hand states simultaneously.

## Entity Link Optimization Complete
- Fixed TODO in ss2_entity_info.rs by implementing HashMap-based link lookups, optimizing performance from O(n) to O(1) for entity template relationship queries while maintaining backward compatibility.
- Learning: Red/green TDD approach works well for performance optimizations - test existing behavior first, then enhance with optimized implementation and verify same results but better performance characteristics.

## Multi-Pass Lighting Step 1 Complete
- Implemented core Light data structure with SpotLight, LightSystem container, and enhanced Scene integration providing foundation for Doom 3-style multi-pass lighting while maintaining backwards compatibility.
- Learning: Complex trait object systems benefit from manual trait bounds (Debug) and careful Clone implementations - derive macros can fail with trait objects requiring custom implementations for proper API design.

## PR State Review Complete
- Fixed compilation error in PR #70 by converting Vec<SceneObject> to Scene using Scene::from_objects() method, addressing type mismatch in engine render calls for both desktop_runtime and oculus_runtime.
- Learning: API changes require systematic updates across all runtimes - backwards compatibility helpers like From<Vec<SceneObject>> for Scene enable gradual migration without breaking existing code paths.

## PR Status Review and Triage Complete
- Resolved critical HashMap import errors in PR #64, identified 2 immediately mergeable PRs (#69, #72), and confirmed 3 others likely ready (#68, #70, #73) with successful CI but pending merge status computation.
- Learning: Systematic PR health checks reveal both immediate wins (missing imports) and complex type system issues (HashMap vs FxHashMap hasher conflicts) - prioritizing quick fixes enables faster iteration on harder problems.

## Compiler Warnings Cleanup Complete
- Systematically resolved compiler warnings by fixing workspace resolver configuration, removing all unused imports, and eliminating clearly unused functions while preserving struct fields needed for file format compatibility.
- Learning: Using TodoWrite tool for tracking multi-step tasks proved invaluable for maintaining focus and demonstrating progress - breaking down "fix warnings" into specific categories (imports, functions, config) enabled systematic completion without scope creep.

## Engine Clippy Warnings Resolution Complete
- Fixed 54+ clippy warnings in engine crate, reducing from 64 to ~10 warnings through systematic refactoring including removing unnecessary return types, fixing trait object references, and improving parameter types.
- Learning: The `cargo clippy --fix` command handles many basic warnings automatically, but trait object references (`&Box<T>` → `&T`) require manual fixes including updating both trait definitions and all implementations for compilation success.

## PR State Review Complete
- Reviewed 6 open PRs: identified PR #31 (FFmpeg integration) as blocking due to major build failures from FFmpeg system dependencies and Android cross-compilation conflicts, while other PRs show healthy CI status.
- Learning: Build validation checklist items in PR descriptions can catch significant issues early - PR #31's unchecked "validate builds on all platforms" revealed system dependency problems that would block deployment.

## VR Teleport System Phase 2 Complete
- Implemented comprehensive arc trajectory physics engine with realistic parabolic motion, landing validation, and visual support foundation for Phase 3, replacing simple forward ray casting with proper kinematic equations.
- Learning: Physics simulation benefits from comprehensive test coverage (6 tests for trajectory, validation, distance limits) and the project's incremental approach enabled clean separation between input detection (Phase 1) and physics calculation (Phase 2) without breaking existing functionality.

## Held Item Save Data Error Handling
- Converted held item instantiation to fail fast on invalid or missing remaps with slot-aware errors and unit coverage to address PR #93 feedback.
- Learning: Targeting `rustfmt` at a single file avoids unintended workspace-wide formatting churn when making quick review follow-ups.
