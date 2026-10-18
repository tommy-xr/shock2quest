// Shared by runtime build scripts. Keep Git inputs out of the gameplay library.
use std::{env, path::PathBuf, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn emit_build_label() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../build-support/version.rs");
    println!("cargo:rerun-if-env-changed=SHOCK2QUEST_RELEASE_VERSION");
    println!("cargo:rerun-if-env-changed=SHOCK2QUEST_GIT_SHA");
    println!("cargo:rerun-if-env-changed=SHOCK2QUEST_BUILD_CHANNEL");
    // Follow the actual git paths: .git can be a file in a linked worktree.
    let mut refs = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    if let Some(branch) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        refs.push(branch);
    }
    for reference in refs {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            let path = PathBuf::from(path);
            // A packed branch has no loose ref yet. Watch its directory so a
            // new commit creating that ref invalidates the embedded SHA too.
            let watched = if reference.starts_with("refs/") {
                path.ancestors().find(|path| path.exists()).unwrap_or(&path)
            } else {
                &path
            };
            if watched.exists() {
                println!("cargo:rerun-if-changed={}", watched.display());
            }
        }
    }
    // Cargo must refresh provenance for source edits as well as new commits.
    // Untracked files are excluded: local reports/assets are not build inputs.
    if let (Some(root), Some(files)) = (
        git(&["rev-parse", "--show-toplevel"]),
        git(&["ls-files", "--full-name", "-z", "--", ":/"]),
    ) {
        for file in files.split('\0').filter(|file| !file.is_empty()) {
            println!(
                "cargo:rerun-if-changed={}",
                PathBuf::from(&root).join(file).display()
            );
        }
    }
    if let Some(index) = git(&["rev-parse", "--git-path", "index"]) {
        println!("cargo:rerun-if-changed={index}");
    }
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .map(|status| (!status.is_empty()).to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    let commit = env::var("SHOCK2QUEST_GIT_SHA")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    let release = env::var("SHOCK2QUEST_RELEASE_VERSION")
        .ok()
        .filter(|value| !value.is_empty());
    // Unreleased builds are named by channel: `dev` locally, `pr` for CI APKs.
    let version = release.clone().unwrap_or_else(|| {
        env::var("SHOCK2QUEST_BUILD_CHANNEL")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "dev".to_owned())
    });
    println!("cargo:rustc-env=SHOCK2QUEST_BUILD_VERSION={version}");
    println!("cargo:rustc-env=SHOCK2QUEST_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=SHOCK2QUEST_BUILD_DIRTY={dirty}");
    let label = match release {
        Some(release) => format!("v{release}"),
        None => version,
    };
    let short_commit: String = commit.chars().take(7).collect();
    println!("cargo:rustc-env=SHOCK2QUEST_BUILD_LABEL={label} | {short_commit}");
}
