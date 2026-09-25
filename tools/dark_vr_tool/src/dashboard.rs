use super::{Device, ROOT, Setting, quest_config};
use anyhow::Result;
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout},
    style::{Color, Stylize},
    widgets::{Block, Paragraph, Wrap},
};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

enum Request {
    Refresh,
    Mission(String),
    Launch,
    Stop,
    Files,
}

pub fn run(serial: Option<String>) -> Result<()> {
    let (requests, inbox) = mpsc::channel();
    let (results, replies) = mpsc::channel();
    // ADB can be slow or wait on a disconnected transport. Keep it off the
    // event thread so the dashboard remains responsive, including Quit.
    thread::spawn(move || {
        for request in inbox {
            let result = (|| -> Result<String> {
                let device = Device::resolve(serial.as_deref())?;
                match request {
                    Request::Refresh => device.status(),
                    Request::Mission(value) => {
                        device.setting(
                            quest_config::MISSION_CONFIG_PATH,
                            Setting::Set { value },
                            true,
                        )?;
                        device.status()
                    }
                    Request::Launch => {
                        device.launch()?;
                        device.status()
                    }
                    Request::Stop => {
                        device.stop()?;
                        device.status()
                    }
                    Request::Files => device.shell(&format!("ls -la {ROOT}")),
                }
            })();
            if results
                .send(result.map_err(|error| format!("{error:#}")))
                .is_err()
            {
                break;
            }
        }
    });
    requests.send(Request::Refresh)?;
    let mut busy = true;
    let mut last_refresh = Instant::now();
    let mut contents = "Connecting to ADB…".to_string();
    let mut editing: Option<String> = None;
    let mut files = false;
    ratatui::run(|terminal| -> Result<()> {
        loop {
            if let Ok(result) = replies.try_recv() {
                contents = result.unwrap_or_else(|error| format!("Connection / operation failed\n\n{error}\n\nCheck USB or wireless ADB and authorize the headset.\nUse cargo dvr devices to list all transports."));
                busy = false;
                last_refresh = Instant::now();
            }
            terminal.draw(|frame| draw(frame, &contents, editing.as_deref(), busy, files))?;
            let mut request = None;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
            {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(());
                }
                if let Some(value) = &mut editing {
                    match key.code {
                        KeyCode::Esc => editing = None,
                        KeyCode::Enter if !busy => {
                            request = Some(Request::Mission(value.clone()));
                            editing = None;
                            files = false;
                        }
                        KeyCode::Backspace => {
                            value.pop();
                        }
                        KeyCode::Char(c) => value.push(c),
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('m') => editing = Some(String::new()),
                        KeyCode::Char('r') if !busy => {
                            files = false;
                            request = Some(Request::Refresh);
                        }
                        KeyCode::Char('l') if !busy => {
                            files = false;
                            request = Some(Request::Launch);
                        }
                        KeyCode::Char('s') if !busy => {
                            files = false;
                            request = Some(Request::Stop);
                        }
                        KeyCode::Char('f') if !busy => {
                            files = true;
                            request = Some(Request::Files);
                        }
                        _ => {}
                    }
                }
            }
            if !busy
                && !files
                && editing.is_none()
                && last_refresh.elapsed() >= Duration::from_secs(5)
            {
                request.get_or_insert(Request::Refresh);
            }
            if let Some(request) = request {
                requests.send(request)?;
                busy = true;
            }
        }
    })
}

fn draw(frame: &mut Frame, contents: &str, editing: Option<&str>, busy: bool, files: bool) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(5),
    ])
    .split(frame.area());
    frame.render_widget(
        Paragraph::new(" SHOCK2QUEST  /  Quest control center")
            .bold()
            .fg(Color::Cyan)
            .block(Block::bordered().title(if busy {
                " Working… "
            } else {
                " Device dashboard · refreshes every 5s "
            })),
        areas[0],
    );
    frame.render_widget(
        Paragraph::new(contents)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(if files { " Device files " } else { " Status " })),
        areas[1],
    );
    let footer = match editing {
        Some(value) => format!("Mission > {value}\nEnter saves · empty removes override · Esc cancels\nUse a .mis filename, debug_* scene, or main_menu; applies on next launch."),
        None => "[m] mission  [l] launch  [s] stop  [f] files  [r] refresh  [q] quit\nBuild & install: cargo dvr deploy   |   Build & run: cargo dvr run\nMore: cargo dvr --help   |   Unset mission boots main_menu".into(),
    };
    frame.render_widget(
        Paragraph::new(footer)
            .fg(Color::Green)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Actions ")),
        areas[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disconnected_and_editor_fit_standard_terminal() {
        let backend = ratatui::backend::TestBackend::new(100, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    "No authorized device",
                    Some("medsci1.mis"),
                    false,
                    false,
                )
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("No authorized device"));
        assert!(text.contains("Mission > medsci1.mis"));
        assert!(text.contains("empty removes override"));
    }
}
