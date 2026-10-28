use anyhow::{Context, Result, bail};
use ci_artifacts::{
    BuildRun, build_runs, cached, ensure_sha, gh, output, pr_head, select_build_run,
};
use clap::Parser;
use std::{path::PathBuf, process::Command};

/// Downloads the desktop binaries CI built for a PR or commit (needs gh)
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// A PR number, or a commit, branch or tag (resolved on GitHub, so push first)
    target: String,
    /// Run this binary with the remaining arguments instead of printing
    /// debug_runtime's path
    #[arg(long, value_parser = ["debug_runtime", "dark_query", "dark_explorer"])]
    run: Option<String>,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, requires = "run")]
    args: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = fetch_runtime(&cli.target)?;
    let Some(bin) = cli.run else {
        println!("{}", runtime.display());
        return Ok(());
    };
    let exe = runtime.with_file_name(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    let mut command = Command::new(&exe);
    command.args(&cli.args);
    // Replace this process, so signals and the pid reach the binary itself.
    #[cfg(unix)]
    let error = std::os::unix::process::CommandExt::exec(&mut command);
    #[cfg(not(unix))]
    let error = match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => error,
    };
    Err(error).with_context(|| format!("could not run {}", exe.display()))
}

/// Uploaded by `.github/workflows/build.yml` for each `runner.os`.
fn runtime_artifact_name(os: &str, sha: &str) -> String {
    format!("shock2quest-runtime-{os}-{sha}")
}

/// The `runner.os` whose CI runtime runs here: build.yml's runners are arm64
/// macOS and x86_64 Linux/Windows.
fn runner_os(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("macOS"),
        ("linux", "x86_64") => Ok("Linux"),
        ("windows", "x86_64") => Ok("Windows"),
        _ => bail!("CI builds no runtime for {os}/{arch}"),
    }
}

/// Files whose changes build.yml's `paths-ignore` skips, so a commit touching
/// only these has no build of its own but the same runtime as its parent.
fn build_ignores(path: &str) -> bool {
    path.ends_with(".md")
        || path.starts_with("tools/shock2-sdk/")
        || path == ".github/workflows/sdk-tests.yml"
}

/// The commit whose build.yml runs hold `sha`'s runtime, with those runs:
/// `sha` itself, or for a skipped commit the nearest ancestor with a run, if
/// every change since is one build.yml ignores.
fn built_commit(sha: &str) -> Result<(String, Vec<BuildRun>)> {
    let runs = build_runs("build.yml", sha)?;
    if !runs.is_empty() {
        return Ok((sha.to_owned(), runs));
    }
    let history = gh(&[
        "api",
        &format!("repos/{{owner}}/{{repo}}/commits?sha={sha}&per_page=31"),
        "--jq",
        ".[].sha",
    ])?;
    for base in history.lines().skip(1) {
        let runs = build_runs("build.yml", base)?;
        if runs.is_empty() {
            continue;
        }
        // A rename's old path counts too; GitHub lists at most 300 files.
        let changed = gh(&[
            "api",
            &format!("repos/{{owner}}/{{repo}}/compare/{base}...{sha}"),
            "--jq",
            r#"if (.files | length) >= 300 then "300+ files"
               else .files[] | .filename, (.previous_filename // empty) end"#,
        ])?;
        if let Some(path) = changed.lines().find(|path| !build_ignores(path)) {
            bail!("{sha} has no build.yml run and changes {path} since {base}, the nearest build");
        }
        eprintln!("{sha} changes only build-ignored files; using {base}'s build");
        return Ok((base.to_owned(), runs));
    }
    bail!("no build.yml run for {sha} or its last 30 ancestors")
}

/// Downloads the runtime CI built for `target` (a PR number, or a commit,
/// branch or tag) into a per-commit temp cache; returns `debug_runtime`'s
/// path. Progress goes to stderr so the path alone can be captured.
fn fetch_runtime(target: &str) -> Result<PathBuf> {
    let os = runner_os(std::env::consts::OS, std::env::consts::ARCH)?;
    // A 7+ digit target is an abbreviated SHA, not a PR.
    let pr = target.parse::<u32>().ok().filter(|_| target.len() < 7);
    let sha = if let Some(number) = pr {
        let (sha, branch) = pr_head(number)?;
        eprintln!("PR #{number} {branch} @ {sha}");
        sha
    } else {
        let sha = gh(&[
            "api",
            &format!("repos/{{owner}}/{{repo}}/commits/{target}"),
            "--jq",
            ".sha",
        ])?
        .trim()
        .to_owned();
        ensure_sha(&sha)?;
        eprintln!("{target} @ {sha}");
        sha
    };
    let (built, runs) = built_commit(&sha)?;
    let run = select_build_run(&runs, "build.yml", &built)?.context("no build.yml run")?;
    let exe = format!("debug_runtime{}", std::env::consts::EXE_SUFFIX);
    // One commit is one build, so a cached runtime cannot be stale.
    cached(
        &format!("shock2quest-runtime/{os}-{built}"),
        &exe,
        |staging| {
            gh(&[
            "run",
            "download",
            &run.id.to_string(),
            "-n",
            &runtime_artifact_name(os, &built),
            "-D",
            &staging.to_string_lossy(),
        ])
        .with_context(|| {
            format!(
                "could not download the runtime from {} (missing if built before CI uploaded one, or older than 7 days)",
                run.url
            )
        })?;
            let tarball = staging.join("runtime.tar.gz");
            output(
                Command::new("tar")
                    .arg("-xzf")
                    .arg(&tarball)
                    .arg("-C")
                    .arg(staging),
            )?;
            std::fs::remove_file(tarball)?;
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_artifacts_match_build_yml() {
        let workflow = include_str!("../../../.github/workflows/build.yml");
        assert!(workflow.contains("name: shock2quest-runtime-${{ runner.os }}-${{ github.sha }}"));
        assert_eq!(
            runtime_artifact_name(runner_os("macos", "aarch64").unwrap(), "abc"),
            "shock2quest-runtime-macOS-abc"
        );
        assert!(runner_os("macos", "x86_64").is_err());
        // build_ignores mirrors paths-ignore.
        for pattern in [
            "'**/*.md'",
            "'tools/shock2-sdk/**'",
            "'.github/workflows/sdk-tests.yml'",
        ] {
            assert!(workflow.contains(pattern), "{pattern}");
        }
        assert!(build_ignores("README.md") && build_ignores("tools/shock2-sdk/src/server.ts"));
        assert!(
            !build_ignores("shock2vr/src/lib.rs") && !build_ignores(".github/workflows/build.yml")
        );
    }
}
