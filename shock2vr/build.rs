use std::{env, path::PathBuf, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SHOCK2QUEST_RELEASE_VERSION");
    println!("cargo:rerun-if-env-changed=SHOCK2QUEST_GIT_SHA");
    // Follow the actual git paths: .git can be a file in a linked worktree.
    let mut refs = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    if let Some(branch) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        refs.push(branch);
    }
    for reference in refs {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            let path = PathBuf::from(path);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    let commit = env::var("SHOCK2QUEST_GIT_SHA")
        .ok()
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    let commit: String = commit.chars().take(7).collect();
    let version = env::var("SHOCK2QUEST_RELEASE_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| format!("v{value}"))
        .unwrap_or_else(|| "dev".to_owned());
    println!("cargo:rustc-env=SHOCK2QUEST_BUILD_LABEL={version} | {commit}");
}
