use crate::{Device, command, payload, validate_assets};
use anyhow::{Context, Result, ensure};
use clap::Args;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Args, Clone, Default)]
pub struct Options {
    /// Local Remaster folder containing sshock2.kpf, mods/ and cutscenes/
    #[arg(long, env = "DARK_ASSET_PATH")]
    pub source: Option<PathBuf>,
    /// Override the bundled release APK
    #[arg(long)]
    pub apk: Option<PathBuf>,
    /// Override the bundled ADB executable (development/testing)
    #[arg(long)]
    pub adb: Option<PathBuf>,
}

#[derive(Debug)]
pub enum Event {
    Stage(&'static str, f32),
    Log(String),
}

impl std::fmt::Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stage(label, _) => writeln!(f, "{label}"),
            Self::Log(text) => write!(f, "{text}"),
        }
    }
}

pub fn run(options: &Options, serial: Option<&str>, emit: impl Fn(Event) + Sync) -> Result<()> {
    emit(Event::Stage("Checking game files", 0.0));
    let source = options
        .source
        .as_ref()
        .context("Choose your System Shock 2 Remaster folder.")?;
    validate_assets(source)?;
    let bundled = if options.adb.is_some() && options.apk.is_some() {
        payload::Payload {
            adb: options.adb.clone().unwrap(),
            apk: options.apk.clone(),
        }
    } else {
        payload::prepare()?
    };
    let apk = options
        .apk
        .as_ref()
        .or(bundled.apk.as_ref())
        .context("Choose an APK to install.")?;
    ensure!(apk.is_file(), "APK not found: {}", apk.display());
    // All local inputs are checked before querying or changing the headset.
    let apk = std::path::absolute(apk)?;
    let adb = options.adb.as_ref().unwrap_or(&bundled.adb);
    emit(Event::Stage("Connecting to headset", 0.05));
    run_command(command(adb).arg("version"), &emit).context(
        "Could not start ADB. Use the packaged installer, or pass --adb for a development build.",
    )?;
    let device = Device::resolve_with_adb(serial, adb.clone())
        .context("Connect and unlock your headset, enable Developer Mode, and accept USB debugging. If several headsets are connected, choose a headset in the installer or pass --serial.")?;
    emit(Event::Log(format!("Headset: {}\n", device.serial)));
    device.stop()?;
    emit(Event::Stage("Installing APK", 0.1));
    run_command(device.adb().args(["install", "-r"]).arg(apk), &emit)?;
    device.copy_assets(source, |name, command| {
        let (label, progress) = match name {
            "sshock2.kpf" => ("Copying base game", 0.3),
            "mods" => ("Copying models and textures", 0.5),
            _ => ("Copying cutscenes", 0.75),
        };
        emit(Event::Stage(label, progress));
        run_command(command, &emit)
    })?;
    emit(Event::Stage("Installation complete", 1.0));
    emit(Event::Log(
        "APK and game files installed. Open Shock2Quest in your headset's app library.\n".into(),
    ));
    Ok(())
}

fn run_command(command: &mut Command, emit: &(impl Fn(Event) + Sync)) -> Result<()> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "could not start {}",
                command.get_program().to_string_lossy()
            )
        })?;
    // Drain both pipes while the process runs: adb writes transfer progress to
    // stderr, and waiting before reading can deadlock when either pipe fills.
    let stdout_pipe = child.stdout.take().unwrap();
    let stderr_pipe = child.stderr.take().unwrap();
    std::thread::scope(|scope| -> Result<()> {
        let pump = |mut pipe: Box<dyn Read + Send>| -> Result<()> {
            let mut bytes = [0; 4096];
            loop {
                let n = pipe.read(&mut bytes)?;
                if n == 0 {
                    return Ok(());
                }
                emit(Event::Log(
                    String::from_utf8_lossy(&bytes[..n]).replace('\r', "\n"),
                ));
            }
        };
        let stdout = scope.spawn(move || pump(Box::new(stdout_pipe)));
        let stderr = scope.spawn(move || pump(Box::new(stderr_pipe)));
        let status = child.wait()?;
        stdout.join().expect("stdout reader panicked")?;
        stderr.join().expect("stderr reader panicked")?;
        ensure!(
            status.success(),
            "ADB operation failed ({status}); see the log above. It is safe to retry after fixing the problem."
        );
        Ok(())
    })
}
