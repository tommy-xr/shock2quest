# Repository Guidelines

See `CLAUDE.md` for the full deep-dive on workflow expectations and engine internals; this guide covers the essentials for a quick start.

## Project Structure & Module Organization
Workspace members in `Cargo.toml` include `dark` (Dark Engine formats), `engine` (rendering + math), and `shock2vr` (gameplay scripts, missions, saves). Runtime wrappers live in `runtimes/desktop_runtime` and `runtimes/oculus_runtime`, with development tools in `tools/dark_viewer`. Retail game assets go in `Data`; docs, utilities, and research stay in `notes`, `tools`, and `references`. Keep build artifacts like `target/` out of commits.

## Build, Test, and Development Commands
- Desktop preview: `cd runtimes/desktop_runtime && cargo run --release` to launch the OpenXR desktop harness.
- Quest build: `cd runtimes/oculus_runtime && source ./set_up_android_sdk.sh && cargo apk run --release` to bundle, install, and start on device.
- Lint and check: `cargo fmt --all` followed by `RUSTFLAGS="-D warnings" cargo check -p shock2vr -p desktop_runtime -p debug_runtime` before pushing. CI compiles with `-D warnings`, so any warning fails the build. Avoid `--workspace` on desktop - Android-only crates (`ndk-sys`, `oboe-sys`) fail to compile for non-Android targets; always scope with `-p`.
- Enable the committed git hooks once per clone (`git config core.hooksPath .githooks`); the pre-push hook runs `cargo fmt --all -- --check` so unformatted Rust can't reach a PR. See DEVELOPMENT.md §2b.

## Coding Style & Naming Conventions
Use the default Rust style (four-space indent, trailing commas, snake_case functions, UpperCamelCase types) enforced by `cargo fmt`. Prefer explicit visibility such as `pub(crate)` and comment unsafe blocks to explain invariants. Align module paths with folder names, register new crates in `Cargo.toml`, and keep asset filenames lowercase with hyphens (for example, `assets/hud-icons/health.png`).

## Testing Guidelines
Run unit tests per package (`cargo test -p shock2vr`, etc. - not `--workspace`, see above). Keep fast checks beside source files under a `tests` module, and heavier scenarios in crate-level `tests/` directories. For behavior that needs a running game (level loading, AI, input), write SDK scenario tests in `tools/shock2-sdk/test/*.e2e.test.ts` gated behind `SHOCK2_E2E=1` (see CLAUDE.md). When updating physics, rendering, or serialization logic, add assertions for any regressions called out in `notes/`. Flag long-running OpenXR device loops with `#[ignore]` and document how to enable them.

## Commit & Pull Request Guidelines
Commits commonly follow a `type: scope` format (`feat: extended lighting pass`, `fix: clippy warnings in engine crate`); keep subjects under 72 characters and scope each commit narrowly. Pull requests should summarize gameplay or engine impact, link related issues, call out required data file updates, and include screenshots or captures when UI/VR interactions change. Confirm `cargo fmt`, `cargo clippy`, and `cargo test` succeed before requesting review, and mention any skipped or ignored tests so reviewers can validate locally.

## Data & VR Runtime Notes
The repository ships without retail `*.mis`, `res/`, or audio banks—developers must copy them into `Data/` before running either runtime. Oculus builds require an unlocked headset plus Android SDK, NDK 24, and `cargo-apk`; ensure `develop.keystore` credentials match `runtimes/oculus_runtime/Cargo.toml`. Use `adb push Data/ /sdcard/shock2quest` to sync content after updates.
