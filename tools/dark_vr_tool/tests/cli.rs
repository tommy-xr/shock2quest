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
import os, sys, json, subprocess, shutil
from pathlib import Path
root = Path(os.environ['DVR_TEST_ROOT'])
a = sys.argv[1:]
with (root / 'calls').open('a') as f: f.write(json.dumps(a) + '\n')
if a == ['version']:
    print('Android Debug Bridge test fixture'); sys.exit(0)
if a == ['devices', '-l']:
    print(os.environ.get('DVR_TEST_DEVICES', 'List of devices attached\nquest device model:Quest_3'))
    sys.exit(0)
if a[0] == 'connect':
    print('connected to ' + a[1]); sys.exit(0)
if a[0] == '-s' and a[2:] == ['get-state']:
    print(os.environ.get('DVR_TEST_WIFI_STATE', 'device')); sys.exit(0)
if a[0] == '-s' and a[2:] == ['shell', 'getprop ro.serialno']:
    print(json.loads(os.environ.get('DVR_TEST_IDENTITIES', '{}')).get(a[1], a[1])); sys.exit(0)
assert a[:2] == ['-s', 'quest'], a
a = a[2:]
if os.environ.get('DVR_TEST_FAIL'):
    print('device disconnected', file=sys.stderr); sys.exit(1)
if a[0] == 'shell' and len(a) == 2:
    script = a[1].replace('/sdcard/shock2quest', str(root / 'data'))
    if script.startswith('am start'): print('Status: ok'); sys.exit(0)
    if script.startswith('am force-stop'): sys.exit(0)
    if script.startswith('ip '): print('    inet 192.168.1.42/24 brd 192.168.1.255 scope global wlan0'); sys.exit(0)
    sys.exit(subprocess.call(['sh', '-c', script]))
if a[0] == 'pull':
    src = Path(a[1].replace('/sdcard/shock2quest', str(root / 'data')))
    Path(a[2]).write_bytes(src.read_bytes()); sys.exit(0)
if a[0] == 'push':
    src = Path(a[1])
    if os.environ.get('DVR_TEST_PUSH_FAIL') == src.name:
        print('no space left on device', file=sys.stderr); sys.exit(1)
    dest = Path(a[2].replace('/sdcard/shock2quest', str(root / 'data'))) / src.name
    if src.is_dir(): shutil.copytree(src, dest, dirs_exist_ok=True)
    else: shutil.copy2(src, dest)
    print('Copied ' + src.name); sys.exit(0)
if a[0] == 'tcpip':
    print('restarting in TCP mode port: ' + a[1]); sys.exit(0)
if a[0] == 'install':
    assert a[1] == '-r' and Path(a[2]).is_file()
    if os.environ.get('DVR_TEST_INSTALL_FAIL'):
        print('INSTALL_FAILED_UPDATE_INCOMPATIBLE', file=sys.stderr); sys.exit(1)
    print('Success'); sys.exit(0)
raise RuntimeError(a)
"#).unwrap();
        fs::set_permissions(adb, fs::Permissions::from_mode(0o755)).unwrap();
        // Intercept the Android build shell: no SDK, signing key or headset needed.
        for name in ["bash", "cargo"] {
            let path = dir.path().join(name);
            fs::write(&path, r#"#!/usr/bin/env python3
import os, sys, json
from pathlib import Path
root = Path(os.environ['DVR_TEST_ROOT'])
with (root / 'calls').open('a') as f: f.write(json.dumps([Path(sys.argv[0]).name] + sys.argv[1:]) + '\n')
if Path(sys.argv[0]).name == 'bash':
    assert sys.argv[1:] == ['-c', 'source ./set_up_android_sdk.sh && cargo apk build --release']
    if os.environ.get('DVR_TEST_BUILD_FAIL'): sys.exit(1)
    apk = root / 'release/apk/shock2quest.apk'
    apk.parent.mkdir(parents=True, exist_ok=True)
    apk.write_text('fake APK')
else:
    assert sys.argv[1:] == ['metadata', '--no-deps', '--format-version', '1']
    print(json.dumps({'target_directory': str(root)}))
"#).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { dir }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dark_vr_tool"));
        cmd.args(args)
            .env_remove("ANDROID_SERIAL")
            .env_remove("DARK_ASSET_PATH")
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
    #[cfg(feature = "installer")]
    fn installer_command(&self) -> Command {
        let source = self.dir.path().join("game ' files");
        fs::create_dir_all(source.join("mods")).unwrap();
        fs::create_dir_all(source.join("cutscenes/enhanced")).unwrap();
        fs::write(source.join("sshock2.kpf"), "game").unwrap();
        fs::write(source.join("mods/test.kpf"), "mod").unwrap();
        fs::write(source.join("cutscenes/enhanced/test.avi"), "video").unwrap();
        let apk = self.dir.path().join("build ' test.apk");
        fs::write(&apk, "apk").unwrap();
        let mut cmd = self.command(&["install"]);
        cmd.arg("--source")
            .arg(source)
            .arg("--apk")
            .arg(apk)
            .arg("--adb")
            .arg(self.dir.path().join("adb"));
        cmd
    }
}

#[cfg(feature = "installer")]
#[test]
fn installer_installs_apk_and_every_asset_before_reporting_success() {
    let h = Harness::new();
    let out = h.installer_command().output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = String::from_utf8_lossy(&out.stdout);
    assert!(log.contains("Installation complete"));
    assert!(log.find("Success").unwrap() < log.find("Copied sshock2.kpf").unwrap());
    assert!(log.find("Copied cutscenes").unwrap() < log.find("Installation complete").unwrap());
    assert!(
        h.dir
            .path()
            .join("data/cutscenes/enhanced/test.avi")
            .is_file()
    );
}

#[cfg(feature = "installer")]
#[test]
fn installer_failures_never_report_success_or_continue_to_next_stage() {
    for (flag, value, expected, forbidden) in [
        (
            "DVR_TEST_INSTALL_FAIL",
            "1",
            "INSTALL_FAILED_UPDATE_INCOMPATIBLE",
            "push",
        ),
        (
            "DVR_TEST_PUSH_FAIL",
            "mods",
            "no space left on device",
            "cutscenes",
        ),
        ("DVR_TEST_DEVICES", "", "Connect and unlock", "install"),
    ] {
        let h = Harness::new();
        let out = h.installer_command().env(flag, value).output().unwrap();
        assert!(!out.status.success());
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(log.contains(expected), "{log}");
        assert!(!log.contains("Installation complete"));
        assert!(
            !fs::read_to_string(h.dir.path().join("calls"))
                .unwrap()
                .contains(forbidden)
        );
    }
}

#[cfg(feature = "installer")]
#[test]
fn installer_rejects_bad_local_inputs_before_device_access() {
    let h = Harness::new();
    let mut command = h.installer_command();
    fs::remove_file(h.dir.path().join("game ' files/sshock2.kpf")).unwrap();
    assert!(!command.output().unwrap().status.success());
    assert!(!h.dir.path().join("calls").exists());
    let mut command = h.installer_command();
    fs::remove_file(h.dir.path().join("build ' test.apk")).unwrap();
    assert!(!command.output().unwrap().status.success());
    assert!(!h.dir.path().join("calls").exists());
}
#[test]
fn build_never_calls_adb_and_run_deploy_build_before_device_selection() {
    for args in [&["build"][..], &["run", "--no-logcat"], &["deploy"]] {
        let h = Harness::new();
        let out = h
            .command(args)
            .env("DVR_TEST_DEVICES", "")
            .output()
            .unwrap();
        assert_eq!(out.status.success(), args[0] == "build");
        assert!(h.dir.path().join("release/apk/shock2quest.apk").is_file());
        let calls = fs::read_to_string(h.dir.path().join("calls")).unwrap();
        assert!(calls.starts_with("[\"bash\""));
        assert_eq!(calls.contains("devices"), args[0] != "build");
    }
}

#[test]
fn failed_build_never_queries_or_mutates_device() {
    let h = Harness::new();
    let out = h
        .command(&["run"])
        .env("DVR_TEST_BUILD_FAIL", "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let calls = fs::read_to_string(h.dir.path().join("calls")).unwrap();
    assert_eq!(calls.lines().count(), 1);
}

#[test]
fn run_builds_installs_then_launches() {
    let h = Harness::new();
    h.run(&["run", "--no-logcat"]);
    let calls = fs::read_to_string(h.dir.path().join("calls")).unwrap();
    assert!(calls.find("install").unwrap() < calls.find("am start").unwrap());
    assert!(!calls.contains("logcat"));
}

#[test]
fn duplicate_transports_prefer_usb_but_distinct_or_unknown_headsets_fail() {
    let devices = "192.168.1.42:5555 device model:Quest_3\nquest device usb:1 model:Quest_3";
    for identity in ["quest", "second", "", "unknown"] {
        let h = Harness::new();
        let out = h
            .command(&["mission", "set", "earth.mis"])
            .env("DVR_TEST_DEVICES", devices)
            .env(
                "DVR_TEST_IDENTITIES",
                serde_json::json!({"192.168.1.42:5555": identity, "quest": "quest"}).to_string(),
            )
            .output()
            .unwrap();
        assert_eq!(out.status.success(), identity == "quest");
        assert_eq!(
            h.dir.path().join("data/vr-mission.txt").is_file(),
            identity == "quest"
        );
        if identity == "quest" {
            let calls = fs::read_to_string(h.dir.path().join("calls")).unwrap();
            let write = calls
                .lines()
                .find(|line| line.contains("vr-mission.txt"))
                .unwrap();
            assert!(write.starts_with(r#"["-s", "quest", "shell", "#));
        }
    }
    let h = Harness::new();
    let out = h
        .command(&["--serial", "quest", "mission", "set", "earth.mis"])
        .env("DVR_TEST_DEVICES", devices)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        !fs::read_to_string(h.dir.path().join("calls"))
            .unwrap()
            .contains("ro.serialno")
    );
}

#[test]
fn assets_copy_recopy_delete_preserve_saves_and_settings() {
    let h = Harness::new();
    let source = h.dir.path().join("retail ' game");
    fs::create_dir_all(source.join("mods")).unwrap();
    fs::create_dir_all(source.join("cutscenes/enhanced")).unwrap();
    fs::write(source.join("sshock2.kpf"), "game").unwrap();
    fs::write(source.join("mods/remaster.kpf"), "mod").unwrap();
    fs::write(source.join("cutscenes/enhanced/intro.avi"), "movie").unwrap();
    h.run(&["mission", "set", "earth.mis"]);
    fs::create_dir(h.dir.path().join("data/saves")).unwrap();
    fs::write(h.dir.path().join("data/saves/slot.sav"), "save").unwrap();
    h.run(&["assets", "copy", source.to_str().unwrap()]);
    assert!(
        h.dir
            .path()
            .join("data/cutscenes/enhanced/intro.avi")
            .is_file()
    );
    fs::write(h.dir.path().join("data/mods/stale.kpf"), "stale").unwrap();
    h.run(&["assets", "copy", source.to_str().unwrap()]);
    assert!(h.dir.path().join("data/mods/stale.kpf").exists());
    let out = h
        .command(&["assets", "recopy"])
        .env("DARK_ASSET_PATH", &source)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!h.dir.path().join("data/mods/stale.kpf").exists());
    assert!(h.dir.path().join("data/mods/remaster.kpf").is_file());
    h.run(&["assets", "delete"]);
    for name in ["sshock2.kpf", "mods", "cutscenes"] {
        assert!(!h.dir.path().join("data").join(name).exists());
    }
    assert!(h.dir.path().join("data/saves/slot.sav").is_file());
    assert!(h.dir.path().join("data/vr-mission.txt").is_file());
}

#[test]
fn invalid_recopy_source_never_touches_device() {
    let h = Harness::new();
    fs::write(h.dir.path().join("sshock2.kpf"), "game").unwrap();
    let out = h
        .command(&["assets", "recopy", h.dir.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!h.dir.path().join("calls").exists());
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
#[test]
fn recordings_list_newest_first_and_pull_with_their_save() {
    let h = Harness::new();
    let dir = h.dir.path().join("data/recordings");
    fs::create_dir_all(&dir).unwrap();
    for (name, frames) in [("rec-100", 2), ("rec-200", 3)] {
        let header = format!(r#"{{"version":1,"scene":"medsci1.mis","save":"{name}.sav"}}"#);
        fs::write(
            dir.join(format!("{name}.jsonl")),
            format!("{header}\n{}", "{}\n".repeat(frames)),
        )
        .unwrap();
        fs::write(dir.join(format!("{name}.sav")), name).unwrap();
    }
    let listing = String::from_utf8(h.run(&["recordings"]).stdout).unwrap();
    let names: Vec<_> = listing
        .lines()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(names, ["rec-200", "rec-100"]);
    assert!(
        listing
            .lines()
            .next()
            .unwrap()
            .contains("3 frames  medsci1.mis")
    );

    let out = h.dir.path().join("pulled");
    h.run(&["pull-recording", "--out", out.to_str().unwrap()]);
    assert!(out.join("rec-200.jsonl").is_file());
    assert_eq!(
        fs::read_to_string(out.join("rec-200.sav")).unwrap(),
        "rec-200"
    );
    h.run(&["pull-recording", "rec-100", "--out", out.to_str().unwrap()]);
    assert!(out.join("rec-100.sav").is_file());
    assert!(
        !h.command(&["pull-recording", "rec-300"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn wifi_switches_usb_device_to_tcp_and_connects() {
    let h = Harness::new();
    let out = h.run(&["wifi"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("192.168.1.42:5555"));
    let calls = fs::read_to_string(h.dir.path().join("calls")).unwrap();
    assert!(calls.contains(r#"["-s", "quest", "tcpip", "5555"]"#));
    assert!(calls.contains(r#"["connect", "192.168.1.42:5555"]"#));
}
#[test]
fn wifi_fails_when_connected_transport_is_offline() {
    let h = Harness::new();
    let out = h
        .command(&["wifi"])
        .env("DVR_TEST_WIFI_STATE", "offline")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("could not connect to 192.168.1.42:5555")
    );
}
