use crate::{command, select_transport};
use anyhow::{Context, Result, bail, ensure};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq)]
pub struct Headset {
    pub serial: String,
    pub model: String,
    pub state: String,
}

impl Headset {
    pub fn tested_model(&self) -> bool {
        self.model.eq_ignore_ascii_case("Quest 3")
    }
}

#[derive(Default)]
pub struct Headsets {
    pub devices: Vec<Headset>,
    pub automatic: Option<String>,
}

pub fn probe(adb: &Path) -> Result<Headsets> {
    let list = query(command(adb).args(["devices", "-l"]))?;
    let automatic = select_transport(&list, None, |serial| {
        query(command(adb).args(["-s", serial, "shell", "getprop ro.serialno"]))
    })
    .ok();
    Ok(Headsets {
        devices: parse(&list),
        automatic,
    })
}

fn parse(list: &str) -> Vec<Headset> {
    list.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            let state = fields.next()?;
            if !matches!(state, "device" | "unauthorized" | "offline" | "no") {
                return None;
            }
            let model = fields
                .find_map(|f| f.strip_prefix("model:"))
                .map(|m| m.replace('_', " "))
                .unwrap_or_else(|| "Unknown device".into());
            Some(Headset {
                serial: serial.into(),
                model,
                state: state.into(),
            })
        })
        .collect()
}

// Probes return a small device listing/property. Bound their lifetime so a stuck
// ADB server cannot leave the connection indicator checking forever.
fn query(command: &mut Command) -> Result<String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not start ADB. Reopen the packaged installer.")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Headset check timed out. Reconnect your headset and try again.");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let result = child.wait_with_output()?;
    ensure!(
        result.status.success(),
        "ADB could not check the headset: {}",
        String::from_utf8_lossy(&result.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_models_and_authorization_without_treating_headers_as_devices() {
        let devices = parse(
            "List of devices attached\n\nusb device usb:1 model:Quest_3\nwifi unauthorized\nold offline model:Quest_2\nno-usb no permissions (user in plugdev group)\n",
        );
        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].model, "Quest 3");
        assert!(devices[0].tested_model());
        assert_eq!(devices[1].state, "unauthorized");
        assert_eq!(devices[1].model, "Unknown device");
        assert!(!devices[1].tested_model());
        assert_eq!(devices[2].state, "offline");
        assert!(!devices[2].tested_model());
        assert_eq!(devices[3].state, "no");
        assert!(parse("List of devices attached\n").is_empty());
    }

    #[test]
    fn quest_3s_and_other_models_show_compatibility_warning() {
        for model in ["Quest_3S", "Quest_2", "Quest_Pro", "Pixel_8"] {
            assert!(!parse(&format!("serial device model:{model}"))[0].tested_model());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_stalled_probe_times_out() {
        let error = query(Command::new("sleep").arg("30")).unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }
}
