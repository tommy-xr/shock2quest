//! Finds and downloads the builds CI uploaded for a commit (needs `gh`).
use anyhow::{Context, Result, bail, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug)]
pub struct BuildRun {
    pub id: u64,
    pub status: String,
    pub conclusion: String,
    pub url: String,
}

/// `gh run list --json databaseId,status,conclusion,url` output.
fn parse_runs(json: &str) -> Result<Vec<BuildRun>> {
    let runs: Vec<serde_json::Value> = serde_json::from_str(json)?;
    runs.iter()
        .map(|run| {
            let text = |key: &str| run[key].as_str().unwrap_or_default().to_owned();
            Ok(BuildRun {
                id: run["databaseId"]
                    .as_u64()
                    .context("run without databaseId")?,
                status: text("status"),
                conclusion: text("conclusion"),
                url: text("url"),
            })
        })
        .collect()
}

/// A successful run of `sha`'s `workflow` (runs newest first), or None when it
/// never ran; a running or failed build is refused, so a stale build is never used.
pub fn select_build_run<'a>(
    runs: &'a [BuildRun],
    workflow: &str,
    sha: &str,
) -> Result<Option<&'a BuildRun>> {
    if let Some(run) = runs
        .iter()
        .find(|run| run.status == "completed" && run.conclusion == "success")
    {
        return Ok(Some(run));
    }
    let Some(latest) = runs.first() else {
        return Ok(None);
    };
    if latest.status != "completed" {
        bail!(
            "{workflow} for {sha} is still {}: {}",
            latest.status,
            latest.url
        );
    }
    bail!(
        "{workflow} for {sha} ended {}: {}",
        latest.conclusion,
        latest.url
    )
}

/// `workflow_file`'s runs for `sha`, newest first.
pub fn build_runs(workflow_file: &str, sha: &str) -> Result<Vec<BuildRun>> {
    parse_runs(&gh(&[
        "run",
        "list",
        "--workflow",
        workflow_file,
        "--commit",
        sha,
        "--json",
        "databaseId,status,conclusion,url",
    ])?)
}

/// Runs `gh` in this repository, returning its stdout.
pub fn gh(args: &[&str]) -> Result<String> {
    output(Command::new("gh").current_dir(repo_root()).args(args))
}

/// PR `number`'s head commit SHA and branch.
pub fn pr_head(number: u32) -> Result<(String, String)> {
    let pr: serde_json::Value = serde_json::from_str(&gh(&[
        "pr",
        "view",
        &number.to_string(),
        "--json",
        "headRefOid,headRefName",
    ])?)?;
    let (Some(sha), Some(branch)) = (pr["headRefOid"].as_str(), pr["headRefName"].as_str()) else {
        bail!("gh pr view {number} reported no head commit")
    };
    ensure_sha(sha)?;
    Ok((sha.to_owned(), branch.to_owned()))
}

/// Fails unless `sha` is a full commit SHA.
pub fn ensure_sha(sha: &str) -> Result<()> {
    ensure!(
        sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "unexpected commit SHA {sha:?}"
    );
    Ok(())
}

/// `<temp>/<key>/<file>`, filled by `download` on a miss. `download`
/// writes into a per-process staging directory, so an interrupted or
/// concurrent download is never reused.
pub fn cached(
    key: &str,
    file: &str,
    download: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    let cache = std::env::temp_dir().join(key);
    let path = cache.join(file);
    if path.is_file() {
        return Ok(path);
    }
    // Not with_extension: release tags contain dots (v0.0.4).
    let staging = std::env::temp_dir().join(format!("{key}.partial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(error) = download(&staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    // A cache the OS temp cleanup partly purged would block the rename.
    if cache.is_dir() && !path.is_file() {
        let _ = std::fs::remove_dir_all(&cache);
    }
    // A concurrent run may have published first; keep its copy.
    if std::fs::rename(&staging, &cache).is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        ensure!(path.is_file(), "could not populate {}", cache.display());
    }
    Ok(path)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `command`, returning its stdout; a non-zero exit is an error.
pub fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .with_context(|| format!("could not execute {:?}", command.get_program()))?;
    ensure!(
        result.status.success(),
        "command failed ({}): {}",
        result.status,
        String::from_utf8_lossy(&result.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(id: u64, status: &str, conclusion: &str) -> BuildRun {
        BuildRun {
            id,
            status: status.into(),
            conclusion: conclusion.into(),
            url: format!("https://example.test/runs/{id}"),
        }
    }
    #[test]
    fn build_selection_never_uses_a_stale_build() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let runs = parse_runs(
            r#"[{"databaseId":2,"status":"completed","conclusion":"failure","url":"u2"},
                {"databaseId":1,"status":"completed","conclusion":"success","url":"u1"}]"#,
        )
        .unwrap();
        assert_eq!(select_build_run(&runs, "W", sha).unwrap().unwrap().id, 1);
        assert!(select_build_run(&[], "W", sha).unwrap().is_none());
        let error = |runs: &[BuildRun]| select_build_run(runs, "W", sha).unwrap_err().to_string();
        assert!(
            error(&[run(3, "in_progress", "")])
                .contains("still in_progress: https://example.test/runs/3")
        );
        assert!(
            error(&[run(4, "completed", "failure")])
                .contains("failure: https://example.test/runs/4")
        );
    }
}
