use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    process::Command,
    thread,
    time::Duration,
};

mod dashboard;
// This module is std-only: share the runtime's boot contract without linking
// the Android runtime (or the game/rendering dependency tree) into a host CLI.
#[allow(dead_code)]
#[path = "../../../runtimes/oculus_runtime/src/quest_config.rs"]
mod quest_config;

const ROOT: &str = "/sdcard/shock2quest";
const PACKAGE: &str = "com.tommybuilds.shock2quest";
const DEBUG_PORT: &str = "/sdcard/shock2quest/debug-port.txt";
const RECORDINGS: &str = "/sdcard/shock2quest/recordings";

#[derive(Parser)]
#[command(about = "Shock2Quest device dashboard and CLI", version)]
struct Cli {
    /// ADB device serial (required when multiple devices are connected)
    #[arg(long, global = true, env = "ANDROID_SERIAL")]
    serial: Option<String>,
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    /// Live device dashboard (also the default in a terminal)
    Tui,
    /// List attached devices, including unauthorized/offline devices
    Devices,
    /// Show device, battery, storage, package and boot configuration
    Status,
    /// Read or change the startup mission; changes take effect on next launch
    #[command(alias = "vr-override")]
    Mission {
        #[command(subcommand)]
        action: Setting,
    },
    /// Configure the loopback debug server; restart app after changing
    DebugPort {
        #[command(subcommand)]
        action: Setting,
    },
    /// Build, install and launch the release APK (follows logcat)
    Run {
        /// Return after launch instead of following logcat
        #[arg(long)]
        no_logcat: bool,
    },
    /// Build and install the release APK without launching, or install --apk
    Deploy {
        #[arg(long)]
        apk: Option<PathBuf>,
    },
    /// Switch the USB-attached headset to ADB over Wi-Fi (lasts until reboot)
    Wifi {
        #[arg(long, default_value_t = 5555)]
        port: u16,
    },
    /// Launch the installed app without rebuilding
    Launch,
    /// Stop the app
    Stop,
    /// List device files; relative paths start in /sdcard/shock2quest
    Ls {
        #[arg(default_value = ROOT)]
        path: String,
    },
    /// Open an interactive ADB shell, or run a remote shell command
    Shell {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Follow application and crash logs
    Logs,
    /// List input recordings on the headset, newest first
    Recordings,
    /// Copy a recording and the save it starts from (default: the newest)
    PullRecording {
        /// e.g. rec-1727390000000
        name: Option<String>,
        #[arg(long, default_value = "recordings")]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum Setting {
    Get,
    /// Empty text deletes the override file
    Set {
        value: String,
    },
    Unset,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let action = cli.command.unwrap_or_else(|| {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            Action::Tui
        } else {
            Action::Status
        }
    });
    if matches!(action, Action::Devices) {
        print!("{}", output(Command::new("adb").args(["devices", "-l"]))?);
        return Ok(());
    }
    if matches!(action, Action::Tui) {
        ensure!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            "dashboard requires a terminal; use status for plain output"
        );
        return dashboard::run(cli.serial);
    }
    let device = Device::resolve(cli.serial.as_deref())?;
    match action {
        Action::Status => print!("{}", device.status()?),
        Action::Mission { action } => {
            device.setting(quest_config::MISSION_CONFIG_PATH, action, true)?
        }
        Action::DebugPort { action } => device.setting(DEBUG_PORT, action, false)?,
        Action::Run { no_logcat } => device.build(true, no_logcat)?,
        Action::Deploy { apk } => {
            let apk = match apk {
                Some(path) => path,
                None => {
                    device.build(false, false)?;
                    apk_path()?
                }
            };
            ensure!(apk.is_file(), "APK not found: {}", apk.display());
            inherit(device.adb().args(["install", "-r"]).arg(apk))?;
        }
        Action::Wifi { port } => device.wifi(port)?,
        Action::Launch => device.launch()?,
        Action::Stop => device.stop()?,
        Action::Ls { path } => {
            let path = if path.starts_with('/') {
                path
            } else {
                format!("{ROOT}/{path}")
            };
            print!("{}", device.shell(&format!("ls -la -- {}", quote(&path)))?);
        }
        Action::Shell { args } => {
            inherit(device.adb().arg("shell").args(args))?;
        }
        Action::Logs => {
            inherit(device.adb().args([
                "logcat",
                "RustStdoutStderr:V",
                "AndroidRuntime:E",
                "DEBUG:E",
                "*:S",
            ]))?;
        }
        Action::Recordings => {
            for recording in device.recordings()? {
                println!(
                    "{}  {:>6} frames  {}",
                    recording.name, recording.frames, recording.scene
                );
            }
        }
        Action::PullRecording { name, out } => device.pull_recording(name.as_deref(), &out)?,
        Action::Tui | Action::Devices => unreachable!(),
    }
    Ok(())
}

fn output(command: &mut Command) -> Result<String> {
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

fn inherit(command: &mut Command) -> Result<()> {
    let status = command.status().context("could not start command")?;
    ensure!(status.success(), "command failed: {status}");
    Ok(())
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn select_serial(list: &str, requested: Option<&str>) -> Result<String> {
    let devices: Vec<_> = list
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            (fields.next()? == "device").then_some(serial)
        })
        .collect();
    if let Some(serial) = requested {
        ensure!(
            devices.contains(&serial),
            "device {serial} is not connected and authorized; run cargo dvr devices"
        );
        return Ok(serial.into());
    }
    ensure!(
        devices.len() == 1,
        "expected one authorized device, found {}; connect/unlock your headset or pass --serial (cargo dvr devices)",
        devices.len()
    );
    Ok(devices[0].into())
}

struct Device {
    serial: String,
}
impl Device {
    fn resolve(serial: Option<&str>) -> Result<Self> {
        let list = output(Command::new("adb").args(["devices", "-l"]))?;
        Ok(Self {
            serial: select_serial(&list, serial)?,
        })
    }
    fn adb(&self) -> Command {
        let mut command = Command::new("adb");
        command.args(["-s", &self.serial]);
        command
    }
    fn shell(&self, script: &str) -> Result<String> {
        output(self.adb().args(["shell", script]))
    }
    fn read(&self, path: &str) -> Result<String> {
        let path = quote(path);
        self.shell(&format!(
            "if [ -e {path} ]; then cat {path}; else printf '(unset)\\n'; fi"
        ))
    }
    fn setting(&self, path: &str, action: Setting, mission: bool) -> Result<()> {
        match action {
            Setting::Get => print!("{}", self.read(path)?),
            Setting::Unset => self.write(path, None)?,
            Setting::Set { value } => {
                let value = validate_setting(&value, mission)?;
                self.write(path, value.as_deref())?;
            }
        }
        Ok(())
    }
    fn write(&self, path: &str, value: Option<&str>) -> Result<()> {
        let script = match value {
            Some(value) => format!(
                "mkdir -p {} && printf '%s\\n' {} > {}",
                quote(ROOT),
                quote(value),
                quote(path)
            ),
            None => format!("rm -f {}", quote(path)),
        };
        self.shell(&script)?;
        Ok(())
    }
    fn recordings(&self) -> Result<Vec<Recording>> {
        // One round trip: each recording's line count, name and header line.
        let dir = quote(RECORDINGS);
        Ok(parse_recordings(&self.shell(&format!(
            "for f in {dir}/rec-*.jsonl; do [ -e \"$f\" ] || continue; \
             printf '%s\\t%s\\t' \"$(wc -l < \"$f\")\" \"$(basename \"$f\" .jsonl)\"; \
             head -n 1 \"$f\"; echo; done"
        ))?))
    }
    fn pull_recording(&self, name: Option<&str>, out: &std::path::Path) -> Result<()> {
        let recordings = self.recordings()?;
        let recording = match name {
            Some(name) => recordings.iter().find(|r| r.name == name),
            None => recordings.first(),
        }
        .with_context(|| match name {
            Some(name) => format!("no recording {name}; run cargo dvr recordings"),
            None => format!("no recordings in {RECORDINGS}"),
        })?;
        std::fs::create_dir_all(out)?;
        for file in [format!("{}.jsonl", recording.name), recording.save.clone()] {
            output(
                self.adb()
                    .arg("pull")
                    .arg(format!("{RECORDINGS}/{file}"))
                    .arg(out.join(&file)),
            )?;
        }
        let path = std::fs::canonicalize(out.join(format!("{}.jsonl", recording.name)))?;
        println!(
            "{}\nRender: cd tools/shock2-sdk && node scripts/hero-shots.mjs --replay {} --name <clip>",
            path.display(),
            quote(&path.to_string_lossy())
        );
        Ok(())
    }
    fn wifi(&self, port: u16) -> Result<()> {
        // A missing wlan0 prints nothing, so it lands in the same error below.
        let addresses = self.shell("ip -f inet addr show wlan0 2>/dev/null || true")?;
        let ip = parse_wlan_ip(&addresses).context("no wlan0 address; is the headset on Wi-Fi?")?;
        output(self.adb().args(["tcpip", &port.to_string()]))?;
        let target = format!("{ip}:{port}");
        // adbd restarts into TCP mode, so the first connects may be refused.
        // `adb connect` can report success for a stale transport; get-state confirms.
        let mut last = String::new();
        for _ in 0..5 {
            thread::sleep(Duration::from_secs(1));
            last = match output(Command::new("adb").args(["connect", &target])) {
                Ok(message) => message,
                Err(error) => error.to_string(),
            };
            let state = output(Command::new("adb").args(["-s", &target, "get-state"]));
            if state.is_ok_and(|state| state.trim() == "device") {
                println!(
                    "Connected: {target}\nUnplug USB, or pass --serial {target} while both are attached."
                );
                return Ok(());
            }
        }
        bail!(
            "could not connect to {target} ({}); check the headset and this machine share a network",
            last.trim()
        )
    }
    fn launch(&self) -> Result<()> {
        let result = self.shell(&format!(
            "am start -S -W -n {PACKAGE}/android.app.NativeActivity"
        ))?;
        ensure!(!result.contains("Error:"), "{result}");
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        self.shell(&format!("am force-stop {PACKAGE}"))?;
        Ok(())
    }
    fn status(&self) -> Result<String> {
        let model = self.shell("getprop ro.product.model")?;
        let battery = self.shell("dumpsys battery")?;
        let battery: Vec<_> = battery
            .lines()
            .filter(|s| s.contains("level:") || s.contains("powered:"))
            .map(str::trim)
            .collect();
        let package = self.shell(&format!("pm path {PACKAGE}"))?;
        let pid = self.shell(&format!("pidof {PACKAGE} || true"))?;
        let storage = self.shell("df -h /sdcard")?;
        let data = self.shell("if [ -f /sdcard/shock2quest/sshock2.kpf ]; then echo Remaster; elif [ -f /sdcard/shock2quest/shock2.gam ]; then echo Legacy; else echo Missing; fi")?;
        Ok(format!(
            "Device   {} ({})\nBattery  {}\nApp      {}\nProcess  {}\nData     {}\nMission  {}\nDebug    {}\n\n{}",
            model.trim(),
            self.serial,
            battery.join(" | "),
            if package.trim().is_empty() {
                "not installed"
            } else {
                "installed"
            },
            if pid.trim().is_empty() {
                "stopped"
            } else {
                pid.trim()
            },
            data.trim(),
            self.read(quest_config::MISSION_CONFIG_PATH)?.trim(),
            self.read(DEBUG_PORT)?.trim(),
            storage
        ))
    }
    fn build(&self, run: bool, no_logcat: bool) -> Result<()> {
        // Arguments are positional bash parameters, never interpolated shell code.
        let mut command = Command::new("bash");
        command
            .current_dir(repo_root().join("runtimes/oculus_runtime"))
            .args([
                "-c",
                "source ./set_up_android_sdk.sh && cargo apk \"$@\"",
                "dark_vr_tool",
                if run { "run" } else { "build" },
                "--release",
                "--device",
                &self.serial,
            ]);
        if no_logcat {
            command.arg("--no-logcat");
        }
        inherit(&mut command)
    }
}

struct Recording {
    name: String,
    scene: String,
    /// The start save, beside the recording.
    save: String,
    frames: usize,
}

/// Parse `recordings`' listing (`<lines>\t<name>\t<header json>` per file),
/// newest first. Names embed their start time in ms, so they sort by it.
/// Unreadable entries (a recording whose header is not flushed yet, a save
/// named outside the directory) are skipped with a warning.
fn parse_recordings(listing: &str) -> Vec<Recording> {
    let mut recordings = Vec::new();
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        match parse_recording(line) {
            Ok(recording) => recordings.push(recording),
            Err(error) => eprintln!("skipping recording: {error:#}"),
        }
    }
    let started = |r: &Recording| {
        r.name
            .trim_start_matches("rec-")
            .parse::<u128>()
            .unwrap_or(0)
    };
    recordings.sort_by_key(|r| std::cmp::Reverse(started(r)));
    recordings
}

fn parse_recording(line: &str) -> Result<Recording> {
    let mut fields = line.splitn(3, '\t');
    let (Some(lines), Some(name), Some(header)) = (fields.next(), fields.next(), fields.next())
    else {
        bail!("unexpected listing line: {line}")
    };
    let header: serde_json::Value =
        serde_json::from_str(header).with_context(|| format!("{name}: unreadable header"))?;
    let save = header["save"].as_str().unwrap_or_default();
    ensure!(
        !save.is_empty() && !save.contains('/') && !save.starts_with('.'),
        "{name}: bad save name {save:?}"
    );
    Ok(Recording {
        name: name.into(),
        scene: header["scene"].as_str().unwrap_or("?").into(),
        save: save.into(),
        // The header is the first line.
        frames: lines.trim().parse::<usize>().unwrap_or(0).saturating_sub(1),
    })
}

/// First IPv4 address in `ip -f inet addr show` output, e.g.
/// `inet 192.168.1.42/24 brd ...` -> `192.168.1.42`.
fn parse_wlan_ip(listing: &str) -> Option<&str> {
    listing.lines().find_map(|line| {
        let address = line.trim().strip_prefix("inet ")?.split('/').next()?;
        (!address.is_empty()).then_some(address)
    })
}

fn validate_setting(value: &str, mission: bool) -> Result<Option<String>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if mission {
        return quest_config::parse_mission(value)
            .map(Some)
            .context("expected a data-relative .mis filename, debug_* scene, or main_menu");
    }
    let port: u16 = value
        .parse()
        .context("debug port must be an integer from 1 to 65535")?;
    ensure!(port != 0, "debug port must be from 1 to 65535");
    Ok(Some(port.to_string()))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn apk_path() -> Result<PathBuf> {
    let raw = output(
        Command::new("cargo")
            .current_dir(repo_root().join("runtimes/oculus_runtime"))
            .args(["metadata", "--no-deps", "--format-version", "1"]),
    )?;
    // Cargo's target directory honors CARGO_TARGET_DIR and .cargo configuration.
    let metadata: serde_json::Value = serde_json::from_str(&raw)?;
    let Some(target) = metadata["target_directory"].as_str() else {
        bail!("cargo metadata did not report target_directory")
    };
    Ok(PathBuf::from(target).join("release/apk/shock2quest.apk"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_selection_never_guesses() {
        let list = "List of devices attached\na device model:Quest\nb unauthorized\nc offline\n";
        assert_eq!(select_serial(list, None).unwrap(), "a");
        assert!(select_serial(list, Some("b")).is_err());
        assert!(select_serial("", None).is_err());
        assert!(select_serial("a device\nb device", None).is_err());
        assert_eq!(select_serial("a device\nb device", Some("b")).unwrap(), "b");
    }
    #[test]
    fn wlan_ip_parses_first_inet_address() {
        let listing = "30: wlan0: <UP> mtu 1500\n    inet 192.168.1.42/24 brd 192.168.1.255 scope global wlan0\n";
        assert_eq!(parse_wlan_ip(listing), Some("192.168.1.42"));
        assert_eq!(parse_wlan_ip(""), None);
    }
    #[test]
    fn settings_validate_and_empty_means_delete() {
        assert_eq!(validate_setting(" \n", true).unwrap(), None);
        assert_eq!(validate_setting("", false).unwrap(), None);
        assert!(validate_setting("earth.mis; reboot", true).is_err());
        assert!(validate_setting("../earth.mis", true).is_err());
        assert_eq!(
            validate_setting(" earth.mis ", true).unwrap(),
            Some("earth.mis".into())
        );
        for port in ["0", "65536", "-1", "abc"] {
            assert!(validate_setting(port, false).is_err());
        }
        assert_eq!(
            validate_setting("8171", false).unwrap(),
            Some("8171".into())
        );
    }
    #[test]
    fn recordings_sort_newest_first_and_reject_escaping_saves() {
        let header =
            |save: &str| format!(r#"{{"version":1,"scene":"medsci1.mis","save":"{save}"}}"#);
        let listing = format!(
            "3\trec-9\t{}\n\n11\trec-10\t{}\n",
            header("rec-9.sav"),
            header("rec-10.sav")
        );
        let recordings = parse_recordings(&listing);
        assert_eq!(recordings[0].name, "rec-10");
        assert_eq!(recordings[0].frames, 10);
        assert_eq!(recordings[1].save, "rec-9.sav");
        assert_eq!(recordings[1].scene, "medsci1.mis");
        for save in ["../x.sav", "/etc/passwd", ""] {
            assert!(parse_recording(&format!("2\trec-1\t{}", header(save))).is_err());
        }
        // An unflushed recording (no header yet) is skipped, not fatal.
        let listing = format!("0\trec-11\t\n{listing}");
        assert_eq!(parse_recordings(&listing).len(), 2);
    }
    #[test]
    fn shell_paths_are_literal() {
        let input = "a' b; $(echo unsafe)";
        assert_eq!(
            output(Command::new("bash").args(["-c", &format!("printf '%s' {}", quote(input))]))
                .unwrap(),
            input
        );
    }
}
