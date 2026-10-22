use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use std::{
    io::{self, IsTerminal},
    path::{Path, PathBuf},
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

use quest_config::{SWITCHES, Switch};
const ROOT: &str = "/sdcard/shock2quest";
const PACKAGE: &str = "com.tommybuilds.shock2quest";
/// Builds that kept saves in app-private storage, which an uninstall deletes:
/// releases v0.0.3 to v0.0.5, and dev builds before the 0.0.0 version reset.
const APP_PRIVATE_SAVE_VERSIONS: [&str; 4] = ["0.0.3", "0.0.4", "0.0.5", "0.1.0"];
const DEBUG_PORT: &str = "/sdcard/shock2quest/debug-port.txt";
const RECORDINGS: &str = "/sdcard/shock2quest/recordings";
/// Written by the in-VR pose tool (`shock2vr::vr_grip::GRIP_OVERRIDES_FILE`).
const GRIP_OVERRIDES: &str = "/sdcard/shock2quest/vr-grip-overrides.json";

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
    /// Copy or remove retail game assets on the headset (preserves saves/settings)
    Assets {
        #[command(subcommand)]
        action: Assets,
    },
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
    /// List or change opt-in feature switch files; changes apply on next launch
    Features {
        #[command(subcommand)]
        action: Option<Features>,
    },
    /// Build, install and launch the release APK (follows logcat)
    Run {
        /// Return after launch instead of following logcat
        #[arg(long)]
        no_logcat: bool,
    },
    /// Build the release APK without requiring ADB or a connected headset
    Build,
    /// Build and install the release APK without launching, or install --apk
    Deploy {
        #[arg(long)]
        apk: Option<PathBuf>,
    },
    /// Install and launch the APK CI built for a PR's head commit (needs gh)
    Pr {
        number: u32,
        /// Also set the startup mission, as `mission set` does
        #[arg(long)]
        mission: Option<String>,
        /// Fail instead of uninstalling an install that cannot be replaced in
        /// place (another key, or a release or older build versioned above it)
        #[arg(long)]
        no_uninstall: bool,
    },
    /// Install and launch a GitHub release's APK, e.g. 0.0.4 (needs gh)
    Release {
        /// e.g. 0.0.4 or v0.0.4
        version: String,
        /// Also set the startup mission, as `mission set` does
        #[arg(long)]
        mission: Option<String>,
        /// Fail instead of uninstalling an install that cannot be replaced in
        /// place (another key, or a higher version)
        #[arg(long)]
        no_uninstall: bool,
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
    /// Remove the installed app; saves and game data under /sdcard/shock2quest stay
    Uninstall,
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
    /// Grip poses saved by the in-VR pose tool
    Grips {
        #[command(subcommand)]
        action: Grips,
    },
}

#[derive(Subcommand)]
enum Grips {
    /// Copy the headset's overrides; merge them with `cargo dx merge-grips <file>`
    Pull {
        #[arg(long, default_value = "vr-grip-overrides.json")]
        out: PathBuf,
    },
    /// Delete the headset's overrides (after merging, so they stop shadowing assets/)
    Clear,
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

#[derive(Subcommand)]
enum Features {
    /// One row per switch with its current device value (the default)
    List,
    /// e.g. `features set multiview on`
    Set { name: String, value: String },
    /// Delete the file, restoring the default
    Unset { name: String },
}

#[derive(Subcommand)]
enum Assets {
    /// Copy sshock2.kpf, mods/ and cutscenes/ from a Remaster installation
    Copy {
        #[arg(env = "DARK_ASSET_PATH")]
        source: PathBuf,
    },
    /// Delete those assets, then copy a fresh set; validates the source first
    Recopy {
        #[arg(env = "DARK_ASSET_PATH")]
        source: PathBuf,
    },
    /// Delete sshock2.kpf, mods/ and cutscenes/ (including custom mods)
    Delete,
}

const GAME_ASSETS: [&str; 3] = ["sshock2.kpf", "mods", "cutscenes"];

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
    // Compile first so a disconnected headset never prevents a local build.
    let apk = match &action {
        Action::Build | Action::Run { .. } | Action::Deploy { apk: None } => {
            build_apk()?;
            Some(apk_path()?)
        }
        Action::Deploy { apk: Some(path) } => Some(path.clone()),
        Action::Pr {
            number, mission, ..
        } => {
            if let Some(mission) = mission {
                validate_setting(mission, true)?;
            }
            Some(fetch_pr_apk(*number)?)
        }
        Action::Release {
            version, mission, ..
        } => {
            if let Some(mission) = mission {
                validate_setting(mission, true)?;
            }
            Some(fetch_release_apk(version)?)
        }
        _ => None,
    };
    if let Some(path) = &apk {
        ensure!(path.is_file(), "APK not found: {}", path.display());
        println!("APK: {}", path.display());
    }
    if matches!(action, Action::Build) {
        return Ok(());
    }
    if let Action::Assets {
        action: Assets::Copy { source } | Assets::Recopy { source },
    } = &action
    {
        ensure!(
            source.join(GAME_ASSETS[0]).is_file(),
            "missing sshock2.kpf in {}",
            source.display()
        );
        for name in &GAME_ASSETS[1..] {
            ensure!(
                source.join(name).is_dir(),
                "missing {name}/ in {}",
                source.display()
            );
        }
    }
    let device = Device::resolve(cli.serial.as_deref())?;
    match action {
        Action::Assets { action } => device.assets(action)?,
        Action::Status => print!("{}", device.status()?),
        Action::Mission { action } => {
            device.setting(quest_config::MISSION_CONFIG_PATH, action, true)?
        }
        Action::DebugPort { action } => device.setting(DEBUG_PORT, action, false)?,
        Action::Features { action } => device.features(action.unwrap_or(Features::List))?,
        Action::Pr {
            mission,
            no_uninstall,
            ..
        }
        | Action::Release {
            mission,
            no_uninstall,
            ..
        } => {
            device.install(&apk.unwrap(), !no_uninstall)?;
            if let Some(value) = mission {
                device.setting(
                    quest_config::MISSION_CONFIG_PATH,
                    Setting::Set { value },
                    true,
                )?;
            }
            device.launch()?;
        }
        Action::Run { .. } | Action::Deploy { .. } => {
            device.install(&apk.unwrap(), true)?;
            if let Action::Run { no_logcat } = action {
                device.launch()?;
                if !no_logcat {
                    device.logs()?;
                }
            }
        }
        Action::Wifi { port } => device.wifi(port)?,
        Action::Launch => device.launch()?,
        Action::Stop => device.stop()?,
        Action::Uninstall => device.uninstall()?,
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
        Action::Logs => device.logs()?,
        Action::Recordings => {
            for recording in device.recordings()? {
                println!(
                    "{}  {:>6} frames  {}",
                    recording.name, recording.frames, recording.scene
                );
            }
        }
        Action::PullRecording { name, out } => device.pull_recording(name.as_deref(), &out)?,
        Action::Grips { action } => match action {
            Grips::Pull { out } => {
                output(device.adb().arg("pull").arg(GRIP_OVERRIDES).arg(&out))
                    .with_context(|| format!("no {GRIP_OVERRIDES} on the headset?"))?;
                println!(
                    "{}\nMerge: cargo dx merge-grips {}",
                    out.display(),
                    out.display()
                );
            }
            Grips::Clear => device.write(GRIP_OVERRIDES, None)?,
        },
        Action::Tui | Action::Devices | Action::Build => unreachable!(),
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

fn authorized_serials(list: &str) -> Vec<&str> {
    list.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            (fields.next()? == "device").then_some(serial)
        })
        .collect()
}

fn select_serial(list: &str, requested: Option<&str>) -> Result<String> {
    let devices = authorized_serials(list);
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
        let devices = authorized_serials(&list);
        if serial.is_none() && devices.len() > 1 {
            // USB and TCP transports expose the same hardware serial. Never
            // collapse devices by model: two Quest 3s must remain ambiguous.
            let mut identity = None;
            for serial in &devices {
                let hardware = Self {
                    serial: (*serial).into(),
                }
                .shell("getprop ro.serialno")
                .context("could not identify transports; pass --serial (cargo dvr devices)")?;
                let hardware = hardware.trim();
                ensure!(
                    !hardware.is_empty()
                        && hardware != "unknown"
                        && identity.as_deref().is_none_or(|id| id == hardware),
                    "multiple or unidentified headsets; pass --serial (cargo dvr devices)"
                );
                identity = Some(hardware.to_owned());
            }
            let preferred = devices
                .iter()
                .find(|serial| {
                    Some(**serial) == identity.as_deref()
                        || list.lines().any(|line| {
                            let mut fields = line.split_whitespace();
                            fields.next() == Some(**serial) && fields.any(|f| f.starts_with("usb:"))
                        })
                })
                .unwrap_or(&devices[0]);
            return Ok(Self {
                serial: (*preferred).into(),
            });
        }
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
    fn features(&self, action: Features) -> Result<()> {
        let path = |switch: &Switch| format!("{ROOT}/{}", switch.file);
        match action {
            Features::List => {
                println!(
                    "{:<17} {:<21} {:<20} {:<8} DEVICE",
                    "NAME", "FILE", "VALUES", "DEFAULT"
                );
                let mut failed = false;
                for switch in SWITCHES {
                    // Prints nothing for a missing file, `+<contents>` otherwise.
                    let file = quote(&path(switch));
                    let current = match self
                        .shell(&format!("[ ! -e {file} ] || {{ printf +; cat {file}; }}"))
                    {
                        Ok(raw) => device_value(switch, raw.strip_prefix('+')),
                        Err(error) => {
                            failed = true;
                            format!("unreadable ({error})")
                        }
                    };
                    println!(
                        "{:<17} {:<21} {:<20} {:<8} {current}",
                        switch.name,
                        switch.file,
                        switch.values.join("|"),
                        switch.default
                    );
                }
                ensure!(!failed, "could not read every switch file");
            }
            Features::Set { name, value } => {
                let switch = feature_switch(&name)?;
                let value = feature_value(switch, &value)?;
                self.write(&path(switch), Some(value))?;
                println!("{} = {value}; applies on next launch", switch.name);
            }
            Features::Unset { name } => {
                let switch = feature_switch(&name)?;
                self.write(&path(switch), None)?;
                println!(
                    "{} = {} (default); applies on next launch",
                    switch.name, switch.default
                );
            }
        }
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
                    "Connected: {target}\nUSB is preferred while attached; unplug to use Wi-Fi."
                );
                return Ok(());
            }
        }
        bail!(
            "could not connect to {target} ({}); check the headset and this machine share a network",
            last.trim()
        )
    }
    fn install(&self, apk: &Path, uninstall: bool) -> Result<()> {
        // -g grants storage up front; a fresh install would otherwise prompt.
        let install = || output(self.adb().args(["install", "-r", "-g"]).arg(apk));
        let installed = match install() {
            Err(error) => {
                let Some(reason) = replace_blocker(&error.to_string()) else {
                    return Err(error);
                };
                ensure!(
                    uninstall,
                    "installed build has {reason}; replacing it needs an uninstall (drop --no-uninstall, or cargo dvr uninstall)"
                );
                let dump = self.shell(&format!("dumpsys package {PACKAGE}"))?;
                let Some(version) = version_name(&dump) else {
                    bail!(
                        "installed build has {reason}, and its version (so where it keeps saves) could not be read; replace it yourself"
                    );
                };
                ensure!(
                    !APP_PRIVATE_SAVE_VERSIONS.contains(&version),
                    "installed build {version} has {reason} and keeps its saves in app-private storage, which an uninstall deletes. To keep them, first install a build with shared-storage saves over it in place (same key, equal or higher version) and launch it once; otherwise uninstall it yourself (cargo dvr uninstall)"
                );
                eprintln!(
                    "Replacing the installed build ({reason}): uninstalling it first. Saves and game data under {ROOT} survive."
                );
                output(self.adb().args(["uninstall", PACKAGE]))?;
                install()?
            }
            Ok(installed) => installed,
        };
        print!("{installed}");
        Ok(())
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
    fn uninstall(&self) -> Result<()> {
        let dump = self.shell(&format!("dumpsys package {PACKAGE}"))?;
        if version_name(&dump).is_some_and(|v| APP_PRIVATE_SAVE_VERSIONS.contains(&v)) {
            eprintln!(
                "Installed build keeps its saves in app-private storage; they are deleted with it."
            );
        }
        print!("{}", output(self.adb().args(["uninstall", PACKAGE]))?);
        Ok(())
    }
    fn assets(&self, action: Assets) -> Result<()> {
        let source = match &action {
            Assets::Copy { source } | Assets::Recopy { source } => {
                Some(std::path::absolute(source)?)
            }
            Assets::Delete => None,
        };
        self.stop()?;
        if matches!(action, Assets::Delete | Assets::Recopy { .. }) {
            let paths = GAME_ASSETS.map(|name| quote(&format!("{ROOT}/{name}")));
            self.shell(&format!("rm -rf -- {}", paths.join(" ")))?;
            println!("Removed sshock2.kpf, mods/ and cutscenes/ from {ROOT}");
        }
        if let Some(source) = source {
            self.shell(&format!("mkdir -p {}", quote(ROOT)))?;
            // Absolute paths cannot be mistaken for adb flags, even if the
            // supplied relative directory starts with '-'.
            for name in GAME_ASSETS {
                inherit(
                    self.adb()
                        .arg("push")
                        .arg(source.join(name))
                        .arg(format!("{ROOT}/")),
                )?;
            }
            println!("Copied assets to {ROOT}; launch the app when ready.");
        }
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
    fn logs(&self) -> Result<()> {
        inherit(self.adb().args([
            "logcat",
            "RustStdoutStderr:V",
            "AndroidRuntime:E",
            "DEBUG:E",
            "*:S",
        ]))
    }
}

fn build_apk() -> Result<()> {
    inherit(
        Command::new("bash")
            .current_dir(repo_root().join("runtimes/oculus_runtime"))
            .args([
                "-c",
                "source ./set_up_android_sdk.sh && cargo apk build --release",
            ]),
    )
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

fn feature_switch(name: &str) -> Result<&'static Switch> {
    SWITCHES
        .iter()
        .find(|switch| switch.name == name)
        .with_context(|| {
            let names: Vec<_> = SWITCHES.iter().map(|switch| switch.name).collect();
            format!("unknown feature {name:?}; expected {}", names.join(", "))
        })
}

/// The runtime silently falls back on unrecognized text, so reject it here.
fn feature_value(switch: &Switch, value: &str) -> Result<&'static str> {
    let value = value.trim();
    switch
        .values
        .iter()
        .copied()
        .find(|allowed| *allowed == value)
        .with_context(|| {
            format!(
                "invalid {} value {value:?}; expected {}",
                switch.name,
                switch.values.join(", ")
            )
        })
}

/// What a switch file (`None`: missing) holds, as the `features` table shows it.
fn device_value(switch: &Switch, contents: Option<&str>) -> String {
    match contents.map(str::trim) {
        None => format!("{} (default)", switch.default),
        Some(value) if switch.values.contains(&value) => value.to_owned(),
        Some(value) => format!("invalid {value:?}"),
    }
}

/// Uploaded by `.github/workflows/build-android.yml`.
fn apk_artifact_name(sha: &str) -> String {
    format!("shock2quest-apk-{sha}")
}

#[derive(Debug)]
struct BuildRun {
    id: u64,
    status: String,
    conclusion: String,
    url: String,
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

/// A successful run of the head commit's build (runs newest first); anything
/// else is refused, so a stale APK is never installed.
fn select_build_run<'a>(runs: &'a [BuildRun], sha: &str, branch: &str) -> Result<&'a BuildRun> {
    if let Some(run) = runs
        .iter()
        .find(|run| run.status == "completed" && run.conclusion == "success")
    {
        return Ok(run);
    }
    let Some(latest) = runs.first() else {
        bail!(
            "no Build Android run for {sha} (paths-ignored pushes skip it); for a same-repo branch, start one: gh workflow run build-android.yml --ref {branch}"
        )
    };
    if latest.status != "completed" {
        bail!(
            "Build Android for {sha} is still {}: {}",
            latest.status,
            latest.url
        );
    }
    bail!(
        "Build Android for {sha} ended {}: {}",
        latest.conclusion,
        latest.url
    )
}

/// The installed `versionName` from `dumpsys package` output.
fn version_name(dumpsys: &str) -> Option<&str> {
    dumpsys
        .lines()
        .find_map(|line| line.trim().strip_prefix("versionName="))
        .filter(|version| !version.is_empty())
}

/// Why a failed `adb install` can only succeed after an uninstall: a
/// signing-key change or a downgrade (a 0.0.0 dev APK over a release, a build
/// signed with another key, or an older 0.1.0 build). Saves live under
/// [`ROOT`], so an uninstall keeps them.
fn replace_blocker(install_error: &str) -> Option<&'static str> {
    if install_error.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") {
        Some("another signing key")
    } else if install_error.contains("INSTALL_FAILED_VERSION_DOWNGRADE") {
        Some("a higher versionCode")
    } else {
        None
    }
}

fn gh(args: &[&str]) -> Result<String> {
    output(Command::new("gh").current_dir(repo_root()).args(args))
}

/// Downloads PR `number`'s CI APK into a per-commit temp cache.
fn fetch_pr_apk(number: u32) -> Result<PathBuf> {
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
    ensure!(
        sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "unexpected head SHA {sha:?}"
    );
    println!("PR #{number} {branch} @ {sha}");
    // One commit is one build, so a cached APK cannot be stale.
    cached_apk(&format!("shock2quest-pr-apk/{sha}"), |staging| {
        let runs = parse_runs(&gh(&[
            "run",
            "list",
            "--workflow",
            "build-android.yml",
            "--commit",
            sha,
            "--json",
            "databaseId,status,conclusion,url",
        ])?)?;
        let run = select_build_run(&runs, sha, branch)?;
        gh(&[
            "run",
            "download",
            &run.id.to_string(),
            "-n",
            &apk_artifact_name(sha),
            "-D",
            &staging.to_string_lossy(),
        ])
        .with_context(|| {
            format!(
                "could not download the APK from {} (missing if built before CI uploaded one, or older than 30 days)",
                run.url
            )
        })?;
        Ok(())
    })
}

/// `0.0.4` or `v0.0.4` -> the release tag `v0.0.4`.
fn release_tag(version: &str) -> Result<String> {
    let number = version.strip_prefix('v').unwrap_or(version);
    ensure!(
        number.split('.').count() == 3
            && number
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())),
        "expected a release version like 0.0.4, got {version:?}"
    );
    Ok(format!("v{number}"))
}

/// Downloads release `version`'s APK into a per-tag temp cache.
fn fetch_release_apk(version: &str) -> Result<PathBuf> {
    let tag = release_tag(version)?;
    println!("Release {tag}");
    // release.yml never replaces a published release's assets, so a cached APK
    // cannot be stale.
    cached_apk(&format!("shock2quest-release-apk/{tag}"), |staging| {
        // v0.0.1 names its APK shock2quest-0.0.1.apk; later releases shock2quest.apk.
        gh(&[
            "release",
            "download",
            &tag,
            "-p",
            "*.apk",
            "-D",
            &staging.to_string_lossy(),
        ])
        .with_context(|| format!("could not download release {tag}'s APK (gh release list)"))?;
        let apks: Vec<_> = std::fs::read_dir(staging)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<io::Result<_>>()?;
        let [apk] = apks.as_slice() else {
            bail!("release {tag} has {} APKs, expected one", apks.len())
        };
        std::fs::rename(apk, staging.join("shock2quest.apk"))?;
        Ok(())
    })
}

/// `<temp>/<key>/shock2quest.apk`, filled by `download` on a miss. `download`
/// writes into a per-process staging directory, so an interrupted or
/// concurrent download is never reused.
fn cached_apk(key: &str, download: impl FnOnce(&Path) -> Result<()>) -> Result<PathBuf> {
    let cache = std::env::temp_dir().join(key);
    let apk = cache.join("shock2quest.apk");
    if apk.is_file() {
        return Ok(apk);
    }
    // Not with_extension: release tags contain dots (v0.0.4).
    let staging = std::env::temp_dir().join(format!("{key}.partial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(error) = download(&staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    // A concurrent run may have published first; keep its copy.
    if std::fs::rename(&staging, &cache).is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        ensure!(apk.is_file(), "could not populate {}", cache.display());
    }
    Ok(apk)
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
    fn features_accept_only_listed_names_and_values() {
        let multiview = feature_switch("multiview").unwrap();
        assert_eq!(feature_value(multiview, " on\n").unwrap(), "on");
        let typo = feature_value(multiview, "yes").unwrap_err().to_string();
        assert!(typo.contains("off, on"), "{typo}");
        let unknown = feature_switch("multivew")
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("simulation-clock, multiview, multimodal, ffr-level"));
        assert_eq!(device_value(multiview, None), "off (default)");
        assert_eq!(device_value(multiview, Some("on\n")), "on");
        assert_eq!(device_value(multiview, Some("On")), "invalid \"On\"");
    }
    #[test]
    fn ffr_switch_matches_the_runtime_parser() {
        // foveation.rs links OpenXR, so check its parser's literals instead.
        let source = include_str!("../../../runtimes/oculus_runtime/src/foveation.rs");
        let levels = &quest_config::FFR_LEVEL;
        for value in levels.values {
            assert!(source.contains(&format!("\"{value}\" => Some(Self::")));
        }
        assert_eq!(
            source.matches("=> Some(Self::").count(),
            levels.values.len()
        );
        let mut default = levels.default.to_owned();
        default[..1].make_ascii_uppercase();
        assert!(source.contains(&format!("#[default]\n    {default},")));
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
    fn run(id: u64, status: &str, conclusion: &str) -> BuildRun {
        BuildRun {
            id,
            status: status.into(),
            conclusion: conclusion.into(),
            url: format!("https://example.test/runs/{id}"),
        }
    }
    #[test]
    fn pr_build_selection_never_installs_a_stale_apk() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(apk_artifact_name(sha), format!("shock2quest-apk-{sha}"));
        let runs = parse_runs(
            r#"[{"databaseId":2,"status":"completed","conclusion":"failure","url":"u2"},
                {"databaseId":1,"status":"completed","conclusion":"success","url":"u1"}]"#,
        )
        .unwrap();
        assert_eq!(select_build_run(&runs, sha, "b").unwrap().id, 1);
        let error = |runs: &[BuildRun]| {
            select_build_run(runs, sha, "feat/x")
                .unwrap_err()
                .to_string()
        };
        assert!(error(&[]).contains("--ref feat/x"));
        assert!(
            error(&[run(3, "in_progress", "")])
                .contains("still in_progress: https://example.test/runs/3")
        );
        assert!(
            error(&[run(4, "completed", "failure")])
                .contains("failure: https://example.test/runs/4")
        );
    }
    #[test]
    fn release_tags_accept_bare_or_v_prefixed_versions() {
        assert_eq!(release_tag("0.0.4").unwrap(), "v0.0.4");
        assert_eq!(release_tag("v0.0.10").unwrap(), "v0.0.10");
        for bad in ["latest", "0.0", "v0.0.4-rc1", "0..4", "../0.0.4", ""] {
            assert!(release_tag(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn version_name_reads_dumpsys() {
        let dump = "Packages:\n  Package [com.tommybuilds.shock2quest] (1a2b):\n    versionCode=16777220 minSdk=26 targetSdk=26\n    versionName=0.0.4\n";
        assert_eq!(version_name(dump), Some("0.0.4"));
        assert_eq!(version_name("Unable to find package"), None);
        assert_eq!(version_name("    versionName="), None);
    }
    #[test]
    fn only_a_signature_change_or_downgrade_needs_an_uninstall() {
        assert_eq!(
            replace_blocker(
                "adb: failed to install x.apk: Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE: signatures do not match]"
            ),
            Some("another signing key")
        );
        assert_eq!(
            replace_blocker(
                "adb: failed to install x.apk: Failure [INSTALL_FAILED_VERSION_DOWNGRADE]"
            ),
            Some("a higher versionCode")
        );
        assert_eq!(
            replace_blocker("Failure [INSTALL_FAILED_INSUFFICIENT_STORAGE]"),
            None
        );
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
