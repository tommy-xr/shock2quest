use crate::{
    headset::{self, Headsets},
    installer::{self, Event},
    payload,
};
use anyhow::Result;
use clap::Args;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(Args, Default)]
pub struct Options {
    #[command(flatten)]
    install: installer::Options,
    /// Render the ready/result screen to PNG and exit (automation)
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Start installation automatically before capturing the result
    #[arg(long, requires = "screenshot")]
    install_now: bool,
}

pub fn run(options: Options, serial: Option<String>) -> Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([760.0, 650.0])
        .with_min_inner_size([620.0, 550.0])
        .with_active(options.screenshot.is_none());
    eframe::run_native(
        "Shock2Quest Installer",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BG;
            visuals.override_text_color = Some(TEXT);
            visuals.extreme_bg_color = BG;
            visuals.faint_bg_color = PANEL;
            visuals.selection.bg_fill = CYAN;
            visuals.selection.stroke = egui::Stroke::new(1.0, BG);
            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
            ] {
                widget.bg_fill = PANEL;
                widget.weak_bg_fill = PANEL;
                widget.bg_stroke = egui::Stroke::new(1.0, LINE);
                widget.corner_radius = egui::CornerRadius::same(2);
            }
            visuals.widgets.hovered.bg_stroke.color = CYAN;
            visuals.widgets.active.bg_stroke.color = CYAN;
            cc.egui_ctx.set_visuals(visuals);
            cc.egui_ctx.all_styles_mut(|style| {
                style.spacing.item_spacing = egui::vec2(10.0, 8.0);
                style.spacing.button_padding = egui::vec2(14.0, 8.0);
            });
            let mut app = InstallerApp::new(options, serial);
            if app.capture_install {
                app.start(cc.egui_ctx.clone());
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("installer window: {error}"))
}

enum Message {
    Update(Event),
    Finished(Result<(), String>),
}

struct InstallerApp {
    source: String,
    apk: String,
    adb: PathBuf,
    serial: String,
    status: String,
    progress: f32,
    success: Option<bool>,
    log: String,
    log_open: bool,
    worker: Option<Receiver<Message>>,
    screenshot: Option<PathBuf>,
    capture_install: bool,
    capture_frames: u32,
    headsets: Headsets,
    probe: Option<Receiver<Result<Headsets, String>>>,
    probe_error: Option<String>,
    next_probe: Instant,
    checked: bool,
}

impl InstallerApp {
    fn new(options: Options, serial: Option<String>) -> Self {
        let mut app = Self {
            source: options
                .install
                .source
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            apk: options
                .install
                .apk
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            adb: options.install.adb.unwrap_or_default(),
            serial: serial.unwrap_or_default(),
            status: "Ready to install".into(),
            progress: 0.0,
            success: None,
            log: String::new(),
            log_open: false,
            worker: None,
            screenshot: options.screenshot,
            capture_install: options.install_now,
            capture_frames: 0,
            headsets: Headsets::default(),
            probe: None,
            probe_error: None,
            next_probe: Instant::now(),
            checked: false,
        };
        match payload::prepare() {
            Ok(payload) => {
                if app.adb.as_os_str().is_empty() {
                    app.adb = payload.adb;
                }
                if app.apk.is_empty()
                    && let Some(apk) = payload.apk
                {
                    app.apk = apk.to_string_lossy().into_owned();
                }
            }
            Err(error) => {
                app.status = "Could not prepare installer files".into();
                app.success = Some(false);
                app.log_open = true;
                app.log = format!("{error:#}\n");
            }
        }
        app
    }

    fn selected(&self) -> Option<&headset::Headset> {
        let serial = if self.serial.is_empty() {
            self.headsets.automatic.as_deref()
        } else {
            Some(self.serial.as_str())
        };
        self.headsets
            .devices
            .iter()
            .find(|device| Some(device.serial.as_str()) == serial)
    }

    fn refresh_headsets(&mut self, ctx: &egui::Context) {
        if let Some(probe) = &self.probe {
            match probe.try_recv() {
                Ok(result) => {
                    self.probe = None;
                    self.checked = true;
                    self.next_probe = Instant::now() + Duration::from_secs(2);
                    match result {
                        Ok(headsets) => {
                            self.headsets = headsets;
                            self.probe_error = None;
                        }
                        Err(error) => {
                            self.headsets = Headsets::default();
                            self.probe_error = Some(error);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.probe = None;
                    self.checked = true;
                    self.headsets = Headsets::default();
                    self.probe_error = Some("Headset check stopped. Please retry.".into());
                    self.next_probe = Instant::now() + Duration::from_secs(2);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.worker.is_none()
            && self.probe.is_none()
            && self.capture_frames == 0
            && Instant::now() >= self.next_probe
        {
            let adb = self.adb.clone();
            let ctx = ctx.clone();
            let (send, recv) = mpsc::channel();
            self.probe = Some(recv);
            std::thread::spawn(move || {
                let _ = send.send(headset::probe(&adb).map_err(|error| format!("{error:#}")));
                ctx.request_repaint();
            });
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn start(&mut self, ctx: egui::Context) {
        let options = installer::Options {
            source: Some(self.source.clone().into()),
            apk: (!self.apk.is_empty()).then(|| self.apk.clone().into()),
            adb: Some(self.adb.clone()),
        };
        let serial = self
            .selected()
            .map(|device| device.serial.clone())
            .or_else(|| (!self.serial.is_empty()).then(|| self.serial.clone()));
        let (send, recv) = mpsc::sync_channel(128);
        self.worker = Some(recv);
        self.log.clear();
        self.log_open = true;
        self.success = None;
        self.progress = 0.0;
        self.status = "Checking installation".into();
        std::thread::spawn(move || {
            let result = installer::run(&options, serial.as_deref(), |event| {
                let _ = send.send(Message::Update(event));
                ctx.request_repaint();
            });
            let _ = send.send(Message::Finished(
                result.map_err(|error| format!("{error:#}")),
            ));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        for _ in 0..256 {
            let Some(worker) = &self.worker else { break };
            match worker.try_recv() {
                Ok(Message::Update(Event::Stage(label, progress))) => {
                    self.status = label.into();
                    self.progress = progress;
                    self.append_log(&format!("\n{label}\n"));
                }
                Ok(Message::Update(Event::Log(text))) => self.append_log(&text),
                Ok(Message::Finished(result)) => {
                    self.worker = None;
                    self.success = Some(result.is_ok());
                    self.log_open = result.is_err();
                    if let Err(error) = result {
                        self.status = "Installation failed".into();
                        self.append_log(&format!("\n{error}\n"));
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.success = Some(false);
                    self.status = "Installer stopped unexpectedly".into();
                    self.append_log("Please retry the installation.\n");
                }
            }
        }
    }

    fn append_log(&mut self, text: &str) {
        self.log.push_str(text);
        if self.log.len() > 64 * 1024 {
            let mut cut = self.log.len() - 48 * 1024;
            while !self.log.is_char_boundary(cut) {
                cut += 1;
            }
            self.log.drain(..cut);
        }
    }

    fn connection_card(&mut self, ui: &mut egui::Ui, busy: bool) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let selected = self.selected();
            let ready = selected.is_some_and(|d| d.state == "device") && self.probe_error.is_none();
            let (title, hint) = if let Some(error) = &self.probe_error {
                ("Connection unavailable".to_owned(), error.clone())
            } else if !self.checked {
                ("Checking for a headset…".into(), "Connect your headset with a USB cable.".into())
            } else if let Some(device) = selected {
                match device.state.as_str() {
                    "device" => (format!("{} · Connected", device.model), String::new()),
                    "unauthorized" => (format!("{} · Authorization needed", device.model), "Put on your headset and allow USB debugging.".into()),
                    _ => (format!("{} · Offline", device.model), "Reconnect and unlock your headset.".into()),
                }
            } else if !self.serial.is_empty() {
                ("Selected headset disconnected".into(), "Reconnect it or choose a different headset below.".into())
            } else if self.headsets.devices.len() == 1 {
                let device = &self.headsets.devices[0];
                if device.state == "unauthorized" {
                    (format!("{} · Authorization needed", device.model), "Put on your headset and allow USB debugging.".into())
                } else {
                    (format!("{} · Offline", device.model), "Reconnect and unlock your headset. Check USB permissions if needed.".into())
                }
            } else if self.headsets.devices.is_empty() {
                ("No headset connected".into(), "Connect by USB and enable Developer Mode on your headset.".into())
            } else {
                ("Choose a headset".into(), "More than one device is connected.".into())
            };
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, if ready { CYAN } else { WARNING });
                ui.label(egui::RichText::new(title).strong());
            });
            if !hint.is_empty() { ui.label(egui::RichText::new(hint).small().color(MUTED)); }
            if let Some(device) = selected {
                ui.label(egui::RichText::new(&device.serial).monospace().small().color(MUTED));
                if device.state == "device" && !device.tested_model() {
                    ui.label(egui::RichText::new("Warning: This APK has only been tested on Quest 3. You may run into performance or playability issues on other devices.").small().color(WARNING));
                }
            }
            if (self.headsets.devices.len() > 1 && self.headsets.automatic.is_none()) || !self.serial.is_empty() {
                ui.add_enabled_ui(!busy, |ui| {
                    egui::ComboBox::from_id_salt("headset_picker").selected_text(if self.serial.is_empty() { "Select a headset" } else { &self.serial }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.serial, String::new(), "Automatic selection");
                        for device in &self.headsets.devices {
                            ui.selectable_value(&mut self.serial, device.serial.clone(), format!("{} — {} ({})", device.model, device.serial, device.state));
                        }
                    });
                });
            }
        });
    }

    fn capture(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_none()
            || self.worker.is_some()
            || !self.checked
            || self.probe.is_some()
        {
            return;
        }
        self.capture_frames += 1;
        if self.capture_frames == 3 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let path = self.screenshot.take().unwrap();
            if let Err(error) = image::save_buffer(
                &path,
                image.as_raw(),
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            ) {
                eprintln!("screenshot failed: {error}");
                std::process::exit(1);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.capture_frames > 300 {
            eprintln!("screenshot did not arrive");
            std::process::exit(1);
        }
        ctx.request_repaint();
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();
        let busy = self.worker.is_some();
        if busy {
            ctx.request_repaint_after(Duration::from_millis(100));
            if ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.append_log("Wait for installation to finish before closing this window.\n");
            }
        }
        self.refresh_headsets(&ctx);
        egui::ScrollArea::vertical()
            .id_salt("installer_form")
            .show(ui, |ui| {
                egui::Frame::new().inner_margin(24.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(egui::RichText::new("SHOCK2").size(28.0).strong());
                        ui.label(egui::RichText::new("QUEST").size(28.0).strong().color(CYAN));
                    });
                    ui.label(
                        egui::RichText::new("SYSTEM SHOCK 2. IN YOUR HANDS.")
                            .monospace()
                            .small()
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    self.connection_card(ui, busy);
                    ui.add_space(4.0);
                    card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            egui::RichText::new("01  GAME FILES")
                                .monospace()
                                .color(CYAN),
                        );
                        ui.label("Choose your System Shock 2 Remaster folder");
                        ui.add_enabled_ui(!busy, |ui| path_picker(ui, &mut self.source));
                        ui.label(
                            egui::RichText::new(
                                "Your purchased copy — containing sshock2.kpf, mods and cutscenes.",
                            )
                            .small()
                            .color(MUTED),
                        );
                    });
                    card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("02  QUEST APP").monospace().color(CYAN));
                        ui.horizontal(|ui| {
                            let label = if self.apk.is_empty() {
                                "Choose an APK".into()
                            } else {
                                PathBuf::from(&self.apk)
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned()
                            };
                            ui.add(egui::Label::new(label).truncate())
                                .on_hover_text(&self.apk);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(!busy, egui::Button::new("Change…"))
                                        .clicked()
                                        && let Some(path) = rfd::FileDialog::new()
                                            .add_filter("Android package", &["apk"])
                                            .pick_file()
                                    {
                                        self.apk = path.to_string_lossy().into_owned();
                                    }
                                },
                            );
                        });
                    });
                    ui.add_space(4.0);
                    let connected = self.probe_error.is_none()
                        && self
                            .selected()
                            .is_some_and(|device| device.state == "device");
                    let ready = connected && !self.source.is_empty() && !self.apk.is_empty();
                    let button = egui::Button::new(
                        egui::RichText::new(if busy {
                            "INSTALLING…"
                        } else {
                            "INSTALL ON QUEST"
                        })
                        .strong()
                        .color(BG),
                    )
                    .fill(CYAN)
                    .min_size(egui::vec2(ui.available_width(), 42.0));
                    if ui.add_enabled(ready && !busy, button).clicked() {
                        self.start(ctx.clone());
                    }
                    if busy || self.success.is_some() {
                        let color = if self.success == Some(false) {
                            WARNING
                        } else {
                            CYAN
                        };
                        ui.horizontal(|ui| {
                            if busy {
                                ui.spinner();
                            }
                            ui.label(egui::RichText::new(&self.status).color(color).strong());
                        });
                        if busy {
                            ui.add(egui::ProgressBar::new(self.progress).fill(CYAN));
                        }
                        if self.success == Some(true) {
                            ui.label(
                                "You're ready. Open Shock2Quest in your headset's app library.",
                            );
                        }
                    } else if !connected {
                        ui.label(
                            egui::RichText::new("Connect and authorize a headset to install.")
                                .small()
                                .color(MUTED),
                        );
                    }
                    ui.label(
                        egui::RichText::new(
                            "Installs the app and game files. Keeps your saves and settings.",
                        )
                        .small()
                        .color(MUTED),
                    );
                    if ui
                        .small_button(if self.log_open {
                            "Hide installation log"
                        } else {
                            "Show installation log"
                        })
                        .clicked()
                    {
                        self.log_open = !self.log_open;
                    }
                    if self.log_open {
                        egui::ScrollArea::vertical()
                            .id_salt("installer_log")
                            .max_height(140.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(if self.log.is_empty() {
                                            "Waiting to begin…"
                                        } else {
                                            &self.log
                                        })
                                        .monospace()
                                        .small(),
                                    )
                                    .wrap(),
                                );
                            });
                    }
                });
            });
        self.capture(&ctx);
    }
}

fn path_picker(ui: &mut egui::Ui, path: &mut String) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(path)
                .desired_width((ui.available_width() - 100.0).max(100.0)),
        );
        if ui.button("Browse…").clicked() {
            let selected = rfd::FileDialog::new().pick_folder();
            if let Some(selected) = selected {
                *path = selected.to_string_lossy().into_owned();
            }
        }
    });
}

// Match the website's dark teal panels, thin borders and cyan accents.
const BG: egui::Color32 = egui::Color32::from_rgb(5, 11, 13);
const PANEL: egui::Color32 = egui::Color32::from_rgb(10, 23, 27);
const LINE: egui::Color32 = egui::Color32::from_rgb(29, 74, 82);
const TEXT: egui::Color32 = egui::Color32::from_rgb(207, 238, 238);
const CYAN: egui::Color32 = egui::Color32::from_rgb(57, 225, 230);
const MUTED: egui::Color32 = egui::Color32::from_rgb(111, 148, 153);
const WARNING: egui::Color32 = egui::Color32::from_rgb(255, 190, 110);

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, LINE))
        .inner_margin(14.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_headset_never_silently_switches_after_disconnect() {
        let mut app = InstallerApp::new(Options::default(), Some("chosen".into()));
        app.headsets = Headsets {
            devices: vec![headset::Headset {
                serial: "other".into(),
                model: "Quest 3".into(),
                state: "device".into(),
            }],
            automatic: Some("other".into()),
        };
        assert!(app.selected().is_none());
        app.serial.clear();
        assert_eq!(app.selected().unwrap().serial, "other");
        app.headsets.automatic = None;
        assert!(app.selected().is_none());
        app.serial = "other".into();
        assert_eq!(app.selected().unwrap().serial, "other");
    }
}
