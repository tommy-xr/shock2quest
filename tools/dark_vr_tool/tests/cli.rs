//! Exercise the real binary against an isolated ADB executable; no headset required.
#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};
use tempfile::TempDir;

struct Harness {
    dir: TempDir,
}
impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let adb = dir.path().join("adb");
        fs::write(&adb, r#"#!/usr/bin/env python3
import os, sys, json, subprocess
from pathlib import Path
root = Path(os.environ['DVR_TEST_ROOT'])
a = sys.argv[1:]
with (root / 'calls').open('a') as f: f.write(json.dumps(a) + '\n')
if a == ['devices', '-l']:
    print(os.environ.get('DVR_TEST_DEVICES', 'List of devices attached\nquest device model:Quest_3'))
    sys.exit(0)
assert a[:2] == ['-s', 'quest'], a
a = a[2:]
if os.environ.get('DVR_TEST_FAIL'):
    print('device disconnected', file=sys.stderr); sys.exit(1)
if a[0] == 'shell' and len(a) == 2:
    script = a[1].replace('/sdcard/shock2quest', str(root / 'data'))
    if script.startswith('am start'): print('Status: ok'); sys.exit(0)
    if script.startswith('am force-stop'): sys.exit(0)
    sys.exit(subprocess.call(['sh', '-c', script]))
if a[0] == 'install':
    assert a[1] == '-r' and Path(a[2]).is_file()
    print('Success'); sys.exit(0)
raise RuntimeError(a)
"#).unwrap();
        fs::set_permissions(adb, fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dark_vr_tool"));
        cmd.args(args)
            .env_remove("ANDROID_SERIAL")
            .env("DVR_TEST_ROOT", self.dir.path())
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
}
#[test]
fn mission_roundtrip_and_both_delete_forms() {
    let h = Harness::new();
    h.run(&["mission", "set", "medsci1.mis"]);
    assert_eq!(h.run(&["mission", "get"]).stdout, b"medsci1.mis\n");
    h.run(&["mission", "set", ""]);
    assert!(!h.dir.path().join("data/vr-mission.txt").exists());
    h.run(&["vr-override", "set", "debug_gloves"]);
    h.run(&["mission", "unset"]);
    assert_eq!(h.run(&["mission", "get"]).stdout, b"(unset)\n");
}
#[test]
fn ports_and_hostile_paths_are_handled_literally() {
    let h = Harness::new();
    h.run(&["debug-port", "set", "8171"]);
    assert_eq!(h.run(&["debug-port", "get"]).stdout, b"8171\n");
    h.run(&["debug-port", "unset"]);
    let res = h.dir.path().join("data/res");
    fs::create_dir_all(&res).unwrap();
    fs::write(res.join("test-resource"), "test").unwrap();
    let out = h.run(&["ls", "res"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("test-resource"));
    let odd = h.dir.path().join("folder '; touch INJECTED; '");
    fs::create_dir(&odd).unwrap();
    fs::write(odd.join("hello"), "test").unwrap();
    let out = h.run(&["ls", odd.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
}
#[test]
fn errors_propagate_and_ambiguous_devices_do_not_mutate() {
    let h = Harness::new();
    let out = h
        .command(&["mission", "set", "earth.mis"])
        .env("DVR_TEST_DEVICES", "quest device\nsecond device")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!h.dir.path().join("data").exists());
    let out = h
        .command(&["mission", "unset"])
        .env("DVR_TEST_FAIL", "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("device disconnected"));
    assert!(
        !h.command(&["mission", "set", "x;reboot.mis"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn install_existing_apk_and_launch_stop_target_selected_device() {
    let h = Harness::new();
    let apk = h.dir.path().join("app with spaces.apk");
    fs::write(&apk, "fake APK").unwrap();
    h.run(&[
        "--serial",
        "quest",
        "deploy",
        "--apk",
        apk.to_str().unwrap(),
    ]);
    h.run(&["launch"]);
    h.run(&["stop"]);
    let calls = fs::read_to_string(h.dir.path().join("calls")).unwrap();
    assert!(
        calls.contains("am start -S -W -n com.tommybuilds.shock2quest/android.app.NativeActivity")
    );
    assert!(calls.contains("am force-stop com.tommybuilds.shock2quest"));
}
