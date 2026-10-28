//! Exercise the real binary against a fake `gh`; no network required.
#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};
use tempfile::TempDir;

// PR 7's head commit has one build.yml run (CI_TEST_RUN_STATUS), and a `docs`
// ref on top of it with no run that changes CI_TEST_CHANGED.
const GH: &str = r#"#!/usr/bin/env python3
import io, os, sys, json, tarfile
from pathlib import Path
root = Path(os.environ['CI_TEST_ROOT'])
a = sys.argv[1:]
with (root / 'calls').open('a') as f: f.write(json.dumps(a) + '\n')
sha = '0123456789abcdef0123456789abcdef01234567'
docs = 'fedcba9876543210fedcba9876543210fedcba98'
api = {'repos/{owner}/{repo}/commits/docs': docs,
       'repos/{owner}/{repo}/commits?sha=' + docs + '&per_page=31': docs + '\n' + sha,
       'repos/{owner}/{repo}/compare/' + sha + '...' + docs: os.environ.get('CI_TEST_CHANGED', 'README.md')}
if a[:3] == ['pr', 'view', '7']:
    print(json.dumps({'headRefOid': sha, 'headRefName': 'feat/x'}))
elif a[0] == 'api':
    print(api[a[1]])
elif a[:2] == ['run', 'list'] and a[a.index('--commit') + 1] == docs:
    print('[]')
elif a[:2] == ['run', 'list']:
    assert a[a.index('--commit') + 1] == sha
    status = os.environ.get('CI_TEST_RUN_STATUS', 'success')
    done = status in ('success', 'failure')
    print(json.dumps([{'databaseId': 42, 'status': 'completed' if done else status,
                       'conclusion': status if done else '', 'url': 'https://runs/42'}]))
elif a[:3] == ['run', 'download', '42']:
    name = a[a.index('-n') + 1]
    assert name.startswith('shock2quest-runtime-') and name.endswith('-' + sha), name
    out = Path(a[a.index('-D') + 1]); out.mkdir(parents=True)
    with tarfile.open(out / 'runtime.tar.gz', 'w:gz') as tar:
        for bin in ['debug_runtime', 'dark_query', 'dark_explorer']:
            script = ('#!/bin/sh\necho %s "$@"\n' % bin).encode()
            info = tarfile.TarInfo(bin); info.size = len(script); info.mode = 0o755
            tar.addfile(info, io.BytesIO(script))
else:
    raise RuntimeError(a)
"#;

struct Harness {
    dir: TempDir,
}
impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let gh = dir.path().join("gh");
        fs::write(&gh, GH).unwrap();
        fs::set_permissions(gh, fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ci_artifacts"));
        cmd.args(args)
            .env("CI_TEST_ROOT", self.dir.path())
            .env("TMPDIR", self.dir.path())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            );
        cmd
    }
    fn run(&self, args: &[&str]) -> Output {
        let out = self.command(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
    fn calls(&self) -> String {
        fs::read_to_string(self.dir.path().join("calls")).unwrap()
    }
}

fn stdout(out: Output) -> String {
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn prints_a_cached_path_and_reuses_a_build_only_past_ignored_changes() {
    let h = Harness::new();
    let runtime = stdout(h.run(&["7"])).trim().to_owned();
    assert!(runtime.ends_with("0123456789abcdef0123456789abcdef01234567/debug_runtime"));
    // A docs-only commit has no run of its own and shares its parent's download.
    assert_eq!(stdout(h.run(&["docs"])).trim(), runtime);
    assert_eq!(h.calls().matches("\"download\"").count(), 1);
    // A source change, a rename out of the source tree or a truncated list is refused.
    for changed in [
        "shock2vr/src/lib.rs",
        "notes.md\nshock2vr/src/notes.rs",
        "300+ files",
    ] {
        let out = h
            .command(&["docs"])
            .env("CI_TEST_CHANGED", changed)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{changed}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("no build.yml run"));
    }
}

#[test]
fn run_execs_a_ci_binary_with_the_remaining_arguments() {
    let h = Harness::new();
    let out = h.run(&["--run", "dark_explorer", "7", "ui", "--belt-card"]);
    assert_eq!(stdout(out), "dark_explorer ui --belt-card\n");
    assert!(
        h.command(&["--run", "desktop_runtime", "7"])
            .output()
            .unwrap()
            .status
            .code()
            == Some(2)
    );
}

#[test]
fn refuses_unfinished_builds() {
    for status in ["in_progress", "failure"] {
        let h = Harness::new();
        let out = h
            .command(&["7"])
            .env("CI_TEST_RUN_STATUS", status)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("https://runs/42"));
        assert!(!h.calls().contains("download"));
    }
}
