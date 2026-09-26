//! Per-frame input recording, for replaying a real play session (organic VR
//! hand motion) in the debug runtime at any resolution and camera.
//!
//! A recording is JSON lines: a [`RecordingHeader`], then one
//! [`RecordedFrame`] per `Game::update` - the raw input, the discrete actions
//! and the frame's dt. Starting a recording also saves the game beside it, so a
//! replay begins from the same state.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{InputAction, InputActionState};
use crate::input_context::InputContext;
use crate::paths;

pub const RECORDING_VERSION: u32 = 1;

/// Where recordings (and their start saves) are written.
pub fn recordings_directory() -> PathBuf {
    paths::data_root().join("recordings")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingHeader {
    pub version: u32,
    /// Scene (mission file or debug scene) the recording started in.
    pub scene: String,
    /// Save written at the first frame, relative to the recording's
    /// directory; `None` when the scene could not be saved.
    pub save: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedFrame {
    pub dt: f32,
    pub input: InputContext,
    pub triggered: Vec<InputAction>,
    pub held: Vec<InputAction>,
}

impl RecordedFrame {
    /// Capture a frame; the recording toggle itself is left out so a replay
    /// never starts or stops a recording.
    pub fn capture(dt: f32, input: &InputContext, actions: &InputActionState) -> Self {
        let keep = |action: &InputAction| *action != InputAction::ToggleInputRecording;
        RecordedFrame {
            dt,
            input: input.clone(),
            triggered: actions.triggered().filter(keep).collect(),
            held: actions.held().filter(keep).collect(),
        }
    }

    /// The frame's actions as the state `Game::update` consumes.
    pub fn actions(&self) -> InputActionState {
        let mut state = InputActionState::new();
        for &action in &self.held {
            state.trigger(action);
            if !self.triggered.contains(&action) {
                state.consume_trigger(action);
            }
        }
        for &action in &self.triggered {
            state.trigger(action);
        }
        state
    }
}

pub struct InputRecorder {
    writer: BufWriter<File>,
    path: PathBuf,
}

impl InputRecorder {
    pub fn create(path: &Path, header: &RecordingHeader) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = BufWriter::new(File::create(path)?);
        writeln!(writer, "{}", serde_json::to_string(header)?)?;
        Ok(InputRecorder {
            writer,
            path: path.to_path_buf(),
        })
    }

    pub fn record(&mut self, frame: &RecordedFrame) -> io::Result<()> {
        writeln!(self.writer, "{}", serde_json::to_string(frame)?)
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
            save: Some("rec.sav".into()),
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

        let mut recorder = InputRecorder::create(&path, &header).unwrap();
        recorder
            .record(&RecordedFrame::capture(1.0 / 72.0, &input, &actions))
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
    }
}
