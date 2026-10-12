use crate::{
    installer::{self, Event},
    payload,
};
use anyhow::Result;
use clap::Args;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::Duration,
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
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.all_styles_mut(|style| {
                style.spacing.item_spacing = egui::vec2(10.0, 10.0);
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
    worker: Option<Receiver<Message>>,
    screenshot: Option<PathBuf>,
    capture_install: bool,
    capture_frames: u32,
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
            worker: None,
            screenshot: options.screenshot,
            capture_install: options.install_now,
            capture_frames: 0,
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
                app.log = format!("{error:#}\n");
            }
        }
        app
    }

    fn start(&mut self, ctx: egui::Context) {
        let options = installer::Options {
            source: Some(self.source.clone().into()),
            apk: (!self.apk.is_empty()).then(|| self.apk.clone().into()),
            adb: Some(self.adb.clone()),
        };
        let serial = (!self.serial.trim().is_empty()).then(|| self.serial.trim().to_owned());
        let (send, recv) = mpsc::sync_channel(128);
        self.worker = Some(recv);
        self.log.clear();
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

    fn capture(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_none() || self.worker.is_some() {
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
        egui::Frame::new().inner_margin(24.0).show(ui, |ui| {
            ui.heading(egui::RichText::new("Shock2Quest").size(30.0).strong());
            ui.label("Install the game on your Quest");
            ui.add_space(8.0);
            ui.label("Connect your headset by USB, enable Developer Mode, and accept the USB debugging prompt inside the headset.");
            ui.separator();
            ui.add_enabled_ui(!busy, |ui| {
                ui.strong("1. Choose your System Shock 2 Remaster folder");
                path_picker(ui, &mut self.source, true);
                ui.small("The folder containing sshock2.kpf, mods, and cutscenes.");
                ui.add_space(4.0);
                ui.strong("2. APK to install");
                path_picker(ui, &mut self.apk, false);
                ui.small("The release APK is included in packaged installers. You can choose another build.");
                egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
                    ui.label("Device serial (optional; select a headset when several are connected)");
                    ui.text_edit_singleline(&mut self.serial);
                });
                ui.add_space(4.0);
                let ready = !self.source.is_empty() && !self.apk.is_empty() && !self.adb.as_os_str().is_empty();
                if ui.add_enabled(ready, egui::Button::new(egui::RichText::new("Install on Quest").strong()).min_size(egui::vec2(180.0, 38.0))).clicked() {
                    self.start(ctx.clone());
                }
            });
            ui.separator();
            let color = match self.success {
                Some(true) => egui::Color32::from_rgb(112, 220, 157),
                Some(false) => egui::Color32::from_rgb(255, 140, 130),
                None => ui.visuals().text_color(),
            };
            ui.horizontal(|ui| {
                if busy { ui.spinner(); }
                ui.label(egui::RichText::new(&self.status).color(color).strong());
            });
            if busy || self.success == Some(true) { ui.add(egui::ProgressBar::new(self.progress)); }
            if self.success == Some(true) {
                ui.label("Open Shock2Quest from your headset's app library.");
            }
            ui.small("Existing saves and settings are kept. Game files are copied into /sdcard/shock2quest.");
            ui.label("Installation log");
            egui::ScrollArea::vertical().max_height(ui.available_height().max(80.0)).stick_to_bottom(true).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(if self.log.is_empty() { "Waiting to begin…" } else { &self.log }).monospace()).wrap());
            });
        });
        self.capture(&ctx);
    }
}

fn path_picker(ui: &mut egui::Ui, path: &mut String, folder: bool) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(path)
                .desired_width((ui.available_width() - 100.0).max(100.0)),
        );
        if ui.button("Browse…").clicked() {
            let dialog = rfd::FileDialog::new();
            let selected = if folder {
                dialog.pick_folder()
            } else {
                dialog.add_filter("Android package", &["apk"]).pick_file()
            };
            if let Some(selected) = selected {
                *path = selected.to_string_lossy().into_owned();
            }
        }
    });
}
