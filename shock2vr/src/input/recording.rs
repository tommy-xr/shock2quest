//! Per-frame input recording, for replaying a real play session (organic VR
//! hand motion) in the debug runtime at any resolution and camera.
//!
//! A recording is JSON lines: a [`RecordingHeader`], then one
//! [`RecordedFrame`] per `Game::update` - the raw input, the discrete actions
//! and the frame's time. Starting a recording also saves the game beside it, so
//! a replay begins from (nearly) the same state.
//!
//! Replay is approximate: a save omits transient state (physics velocities, AI
//! mid-path, latches), some systems use unseeded randomness and AI path
//! queries land on a worker thread, so a replay slowly drifts from the session.
//! Short clips stay close.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{InputAction, InputActionState};
use crate::input_context::InputContext;
use crate::time::Time;
use crate::{PresentationMode, paths};

pub const RECORDING_VERSION: u32 = 1;

/// Flush this often (in frames) so a killed process keeps nearly everything.
const FLUSH_INTERVAL_FRAMES: u32 = 60;

/// Where recordings (and their start saves) are written.
pub fn recordings_directory() -> PathBuf {
    paths::data_root().join("recordings")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingHeader {
    pub version: u32,
    /// Scene (mission file or debug scene) the recording started in.
    pub scene: String,
    /// Save written before the first frame, relative to the recording's
    /// directory.
    pub save: String,
    /// Settings that change what `Game::update` makes of the same input; a
    /// replay must run with the same ones.
    pub presentation: PresentationMode,
    pub experimental: Vec<String>,
    pub glove_forward_cm: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedFrame {
    pub dt: f32,
    /// `Time::total` in seconds: the runtime's clock, which scenes read too.
    pub total: f64,
    pub input: InputContext,
    pub triggered: Vec<InputAction>,
    pub held: Vec<InputAction>,
}

impl RecordedFrame {
    /// Capture a frame. The recording toggle and quick save/load are left out:
    /// a replay must not start recordings or touch the player's quicksave.
    pub fn capture(time: &Time, input: &InputContext, actions: &InputActionState) -> Self {
        let keep = |action: &InputAction| {
            !matches!(
                action,
                InputAction::ToggleInputRecording | InputAction::QuickSave | InputAction::QuickLoad
            )
        };
        RecordedFrame {
            dt: time.elapsed.as_secs_f32(),
            total: time.total.as_secs_f64(),
            input: input.clone(),
            triggered: actions.triggered().filter(keep).collect(),
            held: actions.held().filter(keep).collect(),
        }
    }

    /// The frame's actions as the state `Game::update` consumes. Triggered and
    /// held are independent: a tap can be triggered yet already released.
    pub fn actions(&self) -> InputActionState {
        let mut state = InputActionState::new();
        for &action in self.triggered.iter().chain(&self.held) {
            state.trigger(action);
            if !self.triggered.contains(&action) {
                state.consume_trigger(action);
            }
            if !self.held.contains(&action) {
                state.release(action);
            }
        }
        state
    }
}

pub struct InputRecorder {
    writer: BufWriter<File>,
    path: PathBuf,
    frames: u32,
}

impl InputRecorder {
    pub fn create(path: &Path, header: &RecordingHeader) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // `create_new`: never truncate an earlier recording.
        let mut writer = BufWriter::new(File::options().write(true).create_new(true).open(path)?);
        writeln!(writer, "{}", serde_json::to_string(header)?)?;
        Ok(InputRecorder {
            writer,
            path: path.to_path_buf(),
            frames: 0,
        })
    }

    pub fn record(&mut self, frame: &RecordedFrame) -> io::Result<()> {
        writeln!(self.writer, "{}", serde_json::to_string(frame)?)?;
        self.frames += 1;
        if self.frames % FLUSH_INTERVAL_FRAMES == 0 {
            self.writer.flush()?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<PathBuf> {
        self.writer.flush()?;
        Ok(self.path)
    }
}

pub fn read_recording(path: &Path) -> io::Result<(RecordingHeader, Vec<RecordedFrame>)> {
    let mut lines = BufReader::new(File::open(path)?).lines();
    let header: RecordingHeader = match lines.next() {
        Some(line) => serde_json::from_str(&line?)?,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "empty recording",
            ));
        }
    };
    if header.version != RECORDING_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "recording version {} (expected {RECORDING_VERSION})",
                header.version
            ),
        ));
    }
    let frames = lines
        .map(|line| Ok(serde_json::from_str(&line?)?))
        .collect::<io::Result<Vec<RecordedFrame>>>()?;
    Ok((header, frames))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;

    #[test]
    fn a_recording_round_trips_input_and_actions() {
        let path = std::env::temp_dir().join(format!("rec-test-{}.jsonl", std::process::id()));
        let header = RecordingHeader {
            version: RECORDING_VERSION,
            scene: "medsci1.mis".into(),
            save: "rec.sav".into(),
            presentation: PresentationMode::Vr,
            experimental: vec!["ragdoll".into()],
            glove_forward_cm: 4.0,
        };
        let mut input = InputContext::default();
        input.right_hand.position = vec3(0.1, 1.2, -0.4);
        input.right_hand.trigger_value = 0.8;
        let mut actions = InputActionState::new();
        actions.trigger(InputAction::Reload);
        actions.trigger(InputAction::ToggleInputRecording);
        actions.trigger(InputAction::LeftHandLowerButton);
        actions.clear_triggered();
        actions.trigger(InputAction::Reload);
        // A tap: triggered this frame, already released.
        actions.trigger(InputAction::QuickSave);
        actions.trigger(InputAction::CycleAmmo);
        actions.release(InputAction::CycleAmmo);
        let time = Time {
            elapsed: std::time::Duration::from_secs_f32(1.0 / 72.0),
            total: std::time::Duration::from_secs(3),
        };

        let mut recorder = InputRecorder::create(&path, &header).unwrap();
        recorder
            .record(&RecordedFrame::capture(&time, &input, &actions))
            .unwrap();
        recorder.finish().unwrap();

        let (read_header, frames) = read_recording(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_header, header);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].input.right_hand.position,
            input.right_hand.position
        );
        assert_eq!(frames[0].input.right_hand.trigger_value, 0.8);
        let replayed = frames[0].actions();
        assert!(replayed.just_triggered(InputAction::Reload));
        assert!(replayed.is_held(InputAction::LeftHandLowerButton));
        assert!(!replayed.just_triggered(InputAction::LeftHandLowerButton));
        assert!(!replayed.is_held(InputAction::ToggleInputRecording));
        assert!(!replayed.just_triggered(InputAction::QuickSave));
        assert!(replayed.just_triggered(InputAction::CycleAmmo));
        assert!(!replayed.is_held(InputAction::CycleAmmo));
        assert_eq!(frames[0].total, 3.0);
    }
}
