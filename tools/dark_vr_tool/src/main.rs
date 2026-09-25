use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    process::Command,
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
    fn shell_paths_are_literal() {
        let input = "a' b; $(echo unsafe)";
        assert_eq!(
            output(Command::new("bash").args(["-c", &format!("printf '%s' {}", quote(input))]))
                .unwrap(),
            input
        );
    }
}
