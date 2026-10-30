//! E830-only extraction of the original egui desktop layout.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Rect, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};

use crate::runner::{classify_firmware_files, validate_nand_image, FirmwarePaths, Worker, WorkerEvent};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub show_fps: bool,
    pub fullscreen: bool,
    pub last_import_dir: Option<PathBuf>,
    pub firmware: Option<FirmwarePaths>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { show_fps: true, fullscreen: false, last_import_dir: None, firmware: None }
    }
}

impl Settings {
    pub fn load(root: &std::path::Path) -> Self {
        std::fs::read(root.join("e830-config.json")).ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }
}

#[derive(PartialEq)]
enum Screen { Library, Settings, Run }

enum FileDialogEvent {
    Firmware(Result<FirmwarePaths, String>),
    Nand(Option<PathBuf>),
}

pub struct E830Launcher {
    root: PathBuf,
    config: Settings,
    config_draft: Option<Settings>,
    screen: Screen,
    worker: Option<Worker>,
    events_tx: Sender<FileDialogEvent>,
    events_rx: Receiver<FileDialogEvent>,
    dialog_open: bool,
    running: bool,
    status: String,
    last_frame_texture: Option<egui::TextureHandle>,
    last_frame_status: Option<String>,
    frame_stats: FrameStats,
}

impl E830Launcher {
    pub fn new(root: PathBuf, mut config: Settings) -> Self {
        if config.firmware.is_none() { config.firmware = FirmwarePaths::bundled(&root); }
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            root, config, config_draft: None, screen: Screen::Library,
            worker: None, events_tx, events_rx, dialog_open: false, running: false,
            status: "Welcome to Samsung E830 Emulator.".into(),
            last_frame_texture: None, last_frame_status: None, frame_stats: FrameStats::default(),
        }
    }

    fn save_settings(&mut self) {
        let result = serde_json::to_vec_pretty(&self.config).map_err(anyhow::Error::from)
            .and_then(|bytes| std::fs::write(self.root.join("e830-config.json"), bytes).map_err(anyhow::Error::from));
        if let Err(error) = result { self.status = format!("Could not save settings: {error}"); }
    }

    pub fn run_selected_firmware(&mut self) {
        if let Some(paths) = self.config.firmware.clone() { self.spawn_e830_run(paths); }
    }

    fn stop(&mut self) {
        if let Some(mut worker) = self.worker.take() { worker.stop(); }
        self.running = false;
    }

    fn spawn_e830_run(&mut self, paths: FirmwarePaths) {
        if let Some(nand) = paths.onenand_path() {
            if let Err(error) = validate_nand_image(&nand) {
                self.status = error;
                return;
            }
        }
        self.stop();
        self.config.last_import_dir = paths.cla.parent().map(|path| path.to_path_buf());
        self.config.firmware = Some(paths.clone());
        self.save_settings();
        self.last_frame_texture = None;
        self.last_frame_status = None;
        self.frame_stats = FrameStats::default();
        self.screen = Screen::Run;
        match Worker::start(paths) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.running = true;
                self.status = "Starting Samsung SGH-E830 firmware...".into();
            }
            Err(error) => { self.status = format!("Could not start E830: {error}"); }
        }
    }

    fn spawn_e830_file_dialog(&mut self) {
        if self.dialog_open { return; }
        self.dialog_open = true;
        let directory = self.config.last_import_dir.clone().unwrap_or_else(|| self.root.join("firmware"));
        let sender = self.events_tx.clone();
        std::thread::spawn(move || {
            let selection = rfd::FileDialog::new()
                .set_title("Select Samsung SGH-E830 firmware files")
                .add_filter("E830 firmware (.cla / .nsf / .tfs)", &["cla", "CLA", "nsf", "NSF", "tfs", "TFS"])
                .set_directory(directory).pick_files();
            let result = selection.map(classify_firmware_files).unwrap_or_else(|| Err("cancelled".into()));
            let _ = sender.send(FileDialogEvent::Firmware(result));
        });
        self.status = "Select the matching .cla firmware and .nsf EEPROM; .tfs is optional.".into();
    }

    fn spawn_nand_file_dialog(&mut self) {
        if self.dialog_open { return; }
        let Some(paths) = self.config.firmware.as_ref() else { return; };
        self.dialog_open = true;
        let directory = paths.onenand_path().and_then(|path| path.parent().map(PathBuf::from))
            .unwrap_or_else(|| paths.cla.parent().unwrap_or(&self.root).to_path_buf());
        let sender = self.events_tx.clone();
        std::thread::spawn(move || {
            let selection = rfd::FileDialog::new()
                .set_title("Select E830 physical OneNAND image (32 MiB)")
                .add_filter("Physical NAND image", &["bin", "BIN", "img", "IMG", "nand", "NAND"])
                .add_filter("All files", &["*"])
                .set_directory(directory).pick_file();
            let _ = sender.send(FileDialogEvent::Nand(selection));
        });
        self.status = "Select 32 MiB main-only or 1024+32 interleaved NAND; source files are never overwritten.".into();
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events_rx.try_recv() {
            self.dialog_open = false;
            match event {
                FileDialogEvent::Firmware(Ok(mut paths)) => {
                    // Re-selecting the same CLA keeps its chosen NAND; a new
                    // firmware set starts with its own automatic sibling lookup.
                    if let Some(previous) = self.config.firmware.as_ref() {
                        if previous.cla == paths.cla { paths.onenand = previous.onenand.clone(); }
                    }
                    self.spawn_e830_run(paths);
                }
                FileDialogEvent::Firmware(Err(error)) if error == "cancelled" =>
                    self.status = "Firmware selection cancelled.".into(),
                FileDialogEvent::Firmware(Err(error)) =>
                    self.status = format!("E830 firmware selection failed: {error}"),
                FileDialogEvent::Nand(Some(path)) => {
                    if let Err(error) = validate_nand_image(&path) {
                        self.status = error;
                        continue;
                    }
                    self.stop();
                    if let Some(paths) = self.config.firmware.as_mut() { paths.onenand = Some(path); }
                    self.status = "NAND selected. Press Run to restart from reset; the source image is read-only.".into();
                    self.save_settings();
                    self.screen = Screen::Library;
                }
                FileDialogEvent::Nand(None) => self.status = "NAND selection cancelled.".into(),
            }
        }
        let mut latest = None;
        let mut finished = None;
        if let Some(worker) = self.worker.as_ref() {
            while let Ok(event) = worker.events.try_recv() {
                match event {
                    WorkerEvent::Frame(rgb) => latest = Some(rgb),
                    WorkerEvent::Finished(summary) => finished = Some(summary),
                }
            }
        }
        if let Some(rgb) = latest {
            let image = egui::ColorImage::from_rgb([e830::LCD_WIDTH, e830::LCD_HEIGHT], &rgb);
            if let Some(texture) = self.last_frame_texture.as_mut() {
                texture.set(image, egui::TextureOptions::NEAREST);
            } else {
                self.last_frame_texture = Some(ctx.load_texture("e830-fb", image, egui::TextureOptions::NEAREST));
            }
            self.frame_stats.record_frame();
        }
        if let Some(summary) = finished {
            self.stop();
            self.status = summary.clone();
            self.last_frame_status = Some(summary);
        }
    }

    fn ui_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Samsung E830");
            ui.label(RichText::new("SGH-E830 firmware emulator").small().color(Color32::from_gray(160)));
            ui.add_space(16.0);
            if ui.selectable_label(self.screen == Screen::Library, "Library").clicked() { self.screen = Screen::Library; }
            if ui.selectable_label(self.screen == Screen::Settings, "Settings").clicked() {
                self.config_draft = Some(self.config.clone());
                self.screen = Screen::Settings;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!self.dialog_open, egui::Button::new("Run Samsung E830 firmware...")).clicked() {
                    self.spawn_e830_file_dialog();
                }
            });
        });
        ui.separator();
    }

    fn ui_library(&mut self, ui: &mut egui::Ui) {
        let Some(paths) = self.config.firmware.clone() else {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No firmware selected").heading().color(Color32::from_gray(160)));
                ui.add_space(8.0);
                ui.label("Select the .cla firmware and .nsf EEPROM; a matching .tfs is optional.");
                ui.add_space(20.0);
                if ui.button("Run Samsung E830 firmware...").clicked() { self.spawn_e830_file_dialog(); }
            });
            return;
        };
        // Keep the original library card's size, colors, rounding and spacing.
        let size = Vec2::splat(224.0);
        let (card_rect, response) = ui.allocate_exact_size(size, Sense::click());
        let frame = egui::Frame::none().fill(Color32::from_rgb(35, 38, 46))
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(82, 86, 96)))
            .rounding(16.0).inner_margin(0.0);
        ui.painter().add(frame.paint(card_rect));
        let mut card_ui = ui.child_ui(card_rect.shrink(18.0), egui::Layout::top_down(egui::Align::Center));
        card_ui.add_space(28.0);
        card_ui.heading("Samsung SGH-E830");
        card_ui.add_space(16.0);
        card_ui.label(paths.cla.file_name().unwrap_or_default().to_string_lossy());
        card_ui.label("176 × 220 LCD");
        card_ui.add_space(16.0);
        if card_ui.button("Run").clicked() || response.double_clicked() { self.spawn_e830_run(paths); }
        ui.add_space(12.0);
        if let Some(paths) = self.config.firmware.as_ref() {
            match paths.onenand_path() {
                Some(path) => { ui.label(format!("NAND: {}", path.display())); }
                None => { ui.label("NAND: no image (erased flash)"); }
            }
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.dialog_open, egui::Button::new("Select NAND...")).clicked() {
                self.spawn_nand_file_dialog();
            }
            let explicit = self.config.firmware.as_ref().is_some_and(|paths| paths.onenand.is_some());
            if ui.add_enabled(!self.dialog_open && explicit, egui::Button::new("Use automatic NAND")).clicked() {
                self.stop();
                if let Some(paths) = self.config.firmware.as_mut() { paths.onenand = None; }
                self.status = "Automatic NAND selected: CLA-stem.onenand.bin beside the firmware, or erased flash.".into();
                self.save_settings();
            }
        });
        ui.label(RichText::new("32 MiB main-only or 1024+32 per page. TFS is not a NAND image.").small());
        if self.running {
            ui.add_space(12.0);
            if ui.button("Show run output").clicked() { self.screen = Screen::Run; }
        }
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(8.0);
        let Some(mut draft) = self.config_draft.take() else { return; };
        egui::Grid::new("settings_grid").num_columns(2).spacing(Vec2::new(12.0, 8.0)).show(ui, |ui| {
            ui.label("Show FPS overlay"); ui.checkbox(&mut draft.show_fps, ""); ui.end_row();
            ui.label("Borderless fullscreen");
            if ui.checkbox(&mut draft.fullscreen, "").changed() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(draft.fullscreen));
            }
            ui.end_row();
        });
        ui.add_space(12.0);
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| { save = ui.button("Save").clicked(); cancel = ui.button("Cancel").clicked(); });
        if save {
            self.config = draft;
            self.status = "Settings saved.".into();
            self.save_settings();
            self.screen = Screen::Library;
        } else if cancel {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.config.fullscreen));
            self.screen = Screen::Library;
        } else { self.config_draft = Some(draft); }
    }

    fn ui_run(&mut self, ui: &mut egui::Ui) {
        ui.heading("Run output");
        ui.add_space(8.0);
        if self.running { ui.label("Running Samsung SGH-E830…"); }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Fullscreen (F11)").clicked() { toggle_fullscreen(ui.ctx()); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Back to library").clicked() { self.stop(); self.screen = Screen::Library; }
            });
        });
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            self.ui_run_screen(ui);
            ui.add_space(12.0);
            ui.label("Experimental phone firmware model; keypad input is not emulated yet.");
        });
        ui.add_space(8.0);
        if let Some(status) = self.last_frame_status.as_ref() {
            ui.label(RichText::new(status).small().color(Color32::from_gray(170)));
        }
    }

    fn ui_run_screen(&mut self, ui: &mut egui::Ui) {
        let Some(texture) = self.last_frame_texture.clone() else {
            ui.allocate_ui(Vec2::new(352.0, 440.0), |ui| { ui.label("(no framebuffer captured yet — try Run again)"); });
            return;
        };
        let size = texture.size_vec2();
        let available = ui.available_size();
        let scale = 2.0_f32.min(available.x / size.x).min(available.y / size.y).max(0.1);
        let (rect, _) = ui.allocate_exact_size(size * scale, Sense::hover());
        ui.painter().image(texture.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        if self.config.show_fps {
            let overlay_rect = Rect::from_min_size(rect.min + Vec2::new(6.0, 6.0), Vec2::new(390.0, 24.0));
            ui.painter().rect_filled(overlay_rect, 4.0, Color32::from_black_alpha(190));
            ui.painter().text(overlay_rect.min + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP,
                self.frame_stats.overlay_text(), egui::FontId::monospace(13.0), Color32::LIGHT_GREEN);
        }
    }
}

fn toggle_fullscreen(ctx: &egui::Context) {
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
}

impl eframe::App for E830Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.key_pressed(egui::Key::F11)) { toggle_fullscreen(ctx); }
        self.drain_events(ctx);
        egui::TopBottomPanel::top("top").show(ctx, |ui| self.ui_top_bar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).small().color(Color32::from_gray(140)));
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.screen {
            Screen::Library => self.ui_library(ui),
            Screen::Settings => self.ui_settings(ui),
            Screen::Run => self.ui_run(ui),
        });
        if self.running || self.dialog_open { ctx.request_repaint_after(Duration::from_millis(16)); }
    }
}

// The presentation statistics and overlay match the original desktop frontend.
#[derive(Default)]
struct FrameStats {
    displayed_frames: u64,
    fps: f32,
    window_started_at: Option<Instant>,
    window_frames: u32,
    last_frame_at: Option<Instant>,
    last_frame_ms: Option<f32>,
}

impl FrameStats {
    fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(previous) = self.last_frame_at { self.last_frame_ms = Some(now.duration_since(previous).as_secs_f32() * 1000.0); }
        self.last_frame_at = Some(now);
        self.displayed_frames = self.displayed_frames.saturating_add(1);
        self.window_frames = self.window_frames.saturating_add(1);
        let started_at = *self.window_started_at.get_or_insert(now);
        let elapsed = now.duration_since(started_at);
        if elapsed >= Duration::from_secs(1) {
            self.fps = self.window_frames as f32 / elapsed.as_secs_f32();
            self.window_frames = 0;
            self.window_started_at = Some(now);
        }
    }

    fn overlay_text(&self) -> String {
        let last_ms = self.last_frame_ms.unwrap_or(0.0);
        let since_ms = self.last_frame_at.map(|time| time.elapsed().as_secs_f32() * 1000.0).unwrap_or(0.0);
        format!("FPS {:.1}  Frames {}  Last {:.0}ms  Since {:.0}ms", self.fps, self.displayed_frames, last_ms, since_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher() -> E830Launcher {
        let config: Settings = serde_json::from_str(r#"{
            "firmware":{"cla":"test.cla","eeprom":"test.nsf","tfs":null,"onenand":"selected.bin"}
        }"#).unwrap();
        E830Launcher::new(std::env::temp_dir(), config)
    }

    #[test]
    fn settings_round_trip_preserves_selected_nand() {
        let app = launcher();
        let settings: Settings = serde_json::from_slice(&serde_json::to_vec(&app.config).unwrap()).unwrap();
        assert_eq!(settings.firmware.unwrap().onenand, Some("selected.bin".into()));
        let old: Settings = serde_json::from_str(r#"{"firmware":{"cla":"test.cla","eeprom":"test.nsf","tfs":null}}"#).unwrap();
        assert!(old.firmware.unwrap().onenand.is_none());
    }

    #[test]
    fn cancelled_nand_dialog_keeps_existing_selection() {
        let mut app = launcher();
        app.dialog_open = true;
        app.events_tx.send(FileDialogEvent::Nand(None)).unwrap();
        app.drain_events(&egui::Context::default());
        assert!(!app.dialog_open);
        assert_eq!(app.config.firmware.unwrap().onenand, Some("selected.bin".into()));
        assert_eq!(app.status, "NAND selection cancelled.");
    }

    #[test]
    fn invalid_nand_dialog_keeps_existing_selection_and_running_state() {
        let mut app = launcher();
        app.running = true;
        app.dialog_open = true;
        app.events_tx.send(FileDialogEvent::Nand(Some(app.root.clone()))).unwrap();
        app.drain_events(&egui::Context::default());
        assert!(!app.dialog_open);
        assert!(app.running);
        assert_eq!(app.config.firmware.unwrap().onenand, Some("selected.bin".into()));
        assert!(app.status.contains("directory"));
    }
}
