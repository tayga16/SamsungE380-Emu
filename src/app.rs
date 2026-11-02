//! E830-only extraction of the original egui desktop layout.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Rect, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};
use e830::PhoneKey;

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

#[derive(Clone, Copy, Default)]
struct RuntimeMode {
    sim: bool,
    boot_hle: bool,
    rf_hle: bool,
    battery_hle: bool,
}

impl RuntimeMode {
    fn from_environment() -> Self {
        Self {
            sim: std::env::var("E830_SIM_PRESENT").as_deref() == Ok("1"),
            boot_hle: std::env::var_os("E830_DSP_BOOT_HLE").is_some(),
            rf_hle: std::env::var("E830_DSP_RF_HLE").as_deref() == Ok("empty"),
            battery_hle: std::env::var("E830_ANALOG_HLE").as_deref() == Ok("battery"),
        }
    }

    fn label(self) -> String {
        format!("Mode: {} | DSP boot HLE: {} | RF HLE: {} | Battery HLE: {}",
            if self.sim { "experimental SIM" } else { "no SIM" },
            if self.boot_hle { "on" } else { "off" },
            if self.rf_hle { "empty air" } else { "off" },
            if self.battery_hle { "3.7 V" } else { "off" })
    }

    fn notice(self) -> &'static str {
        if self.sim {
            "Experimental SIM boot is incomplete. Startup animation is not a verified SIM standby or app menu."
        } else if self.boot_hle {
            "No-SIM standby (Insert SIM / SOS) is verified with the installed TFS NAND. App-menu navigation is not verified."
        } else {
            "DSP boot HLE is off. Use Run-E830-NoSIM.cmd for the verified no-card boot path."
        }
    }
}

enum FileDialogEvent {
    Firmware(Result<FirmwarePaths, String>),
    Nand(Option<PathBuf>),
}

pub struct E830Launcher {
    root: PathBuf,
    runtime_mode: RuntimeMode,
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
    sent_keys: u32,
}

impl E830Launcher {
    pub fn new(root: PathBuf, mut config: Settings) -> Self {
        if config.firmware.is_none() { config.firmware = FirmwarePaths::bundled(&root); }
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            root, runtime_mode: RuntimeMode::from_environment(), config, config_draft: None, screen: Screen::Library,
            worker: None, events_tx, events_rx, dialog_open: false, running: false,
            status: "Welcome to Samsung E830 Emulator.".into(),
            last_frame_texture: None, last_frame_status: None, frame_stats: FrameStats::default(),
            sent_keys: 0,
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
        self.sent_keys = 0;
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
                    WorkerEvent::InputApplied(summary) => {
                        self.status = format!("Input: {summary}");
                        self.last_frame_status = Some(summary);
                    }
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
        self.ui_runtime_mode(ui);
        ui.add_space(8.0);
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
        self.ui_runtime_mode(ui);
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
            self.ui_phone_keys(ui);
        });
        ui.add_space(8.0);
        if let Some(status) = self.last_frame_status.as_ref() {
            ui.label(RichText::new(status).small().color(Color32::from_gray(170)));
        }
    }

    fn send_phone_keys(&mut self, keys: u32) {
        if keys != self.sent_keys {
            if let Some(worker) = &self.worker { worker.set_phone_keys(keys); }
            self.sent_keys = keys;
        }
    }

    fn ui_runtime_mode(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new(self.runtime_mode.label()).small());
        let colour = if self.runtime_mode.sim { Color32::from_rgb(240, 190, 100) }
            else { Color32::from_gray(170) };
        ui.label(RichText::new(self.runtime_mode.notice()).small().color(colour));
    }

    fn ui_phone_keys(&mut self, ui: &mut egui::Ui) {
        let keyboard = ui.input(|input| phone_keyboard_mask(input));
        let focused = ui.input(|input| input.focused);
        let mut mouse = 0;
        let mut mouse_clicks = 0;
        ui.vertical(|ui| {
            ui.label("Phone keypad");
            ui.add_enabled_ui(self.running && focused, |ui| {
                egui::Grid::new("phone_keypad").spacing(Vec2::new(5.0, 5.0)).show(ui, |ui| {
                    for row in [
                        [("Left soft", PhoneKey::SoftLeft), ("Up", PhoneKey::Up), ("Right soft", PhoneKey::SoftRight)],
                        [("Left", PhoneKey::Left), ("Call", PhoneKey::Ok), ("Right", PhoneKey::Right)],
                        [("+", PhoneKey::Call), ("Down", PhoneKey::Down), ("−", PhoneKey::End)],
                        [("1", PhoneKey::One), ("2 abc", PhoneKey::Two), ("3 def", PhoneKey::Three)],
                        [("4 ghi", PhoneKey::Four), ("5 jkl", PhoneKey::Five), ("6 mno", PhoneKey::Six)],
                        [("7 pqrs", PhoneKey::Seven), ("8 tuv", PhoneKey::Eight), ("9 wxyz", PhoneKey::Nine)],
                        [("*", PhoneKey::Star), ("0", PhoneKey::Zero), ("#", PhoneKey::Hash)],
                    ] {
                        for (label, key) in row {
                            let button = egui::Button::new(label).min_size(Vec2::new(72.0, 35.0))
                                .selected(self.sent_keys & key.bit() != 0);
                            let response = ui.add(button);
                            // A click is reported on the release frame. Treating
                            // `clicked_by` as a held contact delayed release by
                            // another GUI frame while the guest kept running.
                            if response.is_pointer_button_down_on() { mouse |= key.bit(); }
                            if response.clicked_by(egui::PointerButton::Primary) {
                                mouse_clicks |= key.bit();
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Arrows: navigate | Enter: Call\nF1/F2: soft keys | F3/F4: +/−\n0-9: digits | F5/F6: */#").small());
            ui.label(RichText::new("Experimental hardware input.\nSIM/network support is incomplete.").small());
        });
        let keys = if self.running && focused { keyboard | mouse } else { 0 };
        // A fast mouse press and release can both arrive between repaints, so
        // `is_pointer_button_down_on` is never observed. Preserve that short
        // click as an explicit down/up pair. A normally observed held contact
        // has its bit in `sent_keys`, and therefore only takes the release path.
        let click_pulse = missed_click_pulse(self.sent_keys, keys, mouse_clicks);
        if click_pulse != 0 { self.send_phone_keys(keys | click_pulse); }
        self.send_phone_keys(keys);
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
        if self.screen != Screen::Run || !self.running || !ctx.input(|input| input.focused) {
            self.send_phone_keys(0);
        }
        if self.running || self.dialog_open { ctx.request_repaint_after(Duration::from_millis(16)); }
    }
}

fn phone_keyboard_mask(input: &egui::InputState) -> u32 {
    if !input.focused || input.modifiers.ctrl || input.modifiers.alt || input.modifiers.command { return 0; }
    use egui::Key;
    [
        (Key::ArrowUp, PhoneKey::Up), (Key::ArrowDown, PhoneKey::Down),
        (Key::ArrowLeft, PhoneKey::Left), (Key::ArrowRight, PhoneKey::Right),
        (Key::Enter, PhoneKey::Ok), (Key::F1, PhoneKey::SoftLeft),
        (Key::F2, PhoneKey::SoftRight), (Key::F3, PhoneKey::Call),
        (Key::F4, PhoneKey::End), (Key::F5, PhoneKey::Star), (Key::F6, PhoneKey::Hash),
        (Key::Num0, PhoneKey::Zero), (Key::Num1, PhoneKey::One),
        (Key::Num2, PhoneKey::Two), (Key::Num3, PhoneKey::Three),
        (Key::Num4, PhoneKey::Four), (Key::Num5, PhoneKey::Five),
        (Key::Num6, PhoneKey::Six), (Key::Num7, PhoneKey::Seven),
        (Key::Num8, PhoneKey::Eight), (Key::Num9, PhoneKey::Nine),
    ].into_iter().fold(0, |mask, (key, phone)| {
        mask | if input.key_down(key) || input.key_pressed(key) { phone.bit() } else { 0 }
    })
}

fn missed_click_pulse(previous: u32, current: u32, clicks: u32) -> u32 {
    clicks & !previous & !current
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
    fn keyboard_contacts_include_short_taps_and_release_on_focus_loss() {
        let ctx = egui::Context::default();
        let event = |key, pressed| egui::Event::Key {
            key, physical_key: None, pressed, repeat: false, modifiers: Default::default(),
        };
        let mut raw = egui::RawInput::default();
        raw.events = vec![event(egui::Key::Num1, true), event(egui::Key::Num1, false),
            event(egui::Key::ArrowUp, true)];
        ctx.begin_frame(raw);
        assert_eq!(ctx.input(phone_keyboard_mask), PhoneKey::One.bit() | PhoneKey::Up.bit());
        let _ = ctx.end_frame();
        ctx.begin_frame(egui::RawInput::default());
        assert_eq!(ctx.input(phone_keyboard_mask), PhoneKey::Up.bit());
        let _ = ctx.end_frame();
        let mut raw = egui::RawInput::default();
        raw.focused = false;
        ctx.begin_frame(raw);
        assert_eq!(ctx.input(phone_keyboard_mask), 0);
        let _ = ctx.end_frame();
    }

    #[test]
    fn fast_mouse_click_gets_a_pulse_but_normal_release_does_not_repeat() {
        let up = PhoneKey::Up.bit();
        assert_eq!(missed_click_pulse(0, 0, up), up);
        assert_eq!(missed_click_pulse(0, up, up), 0);
        assert_eq!(missed_click_pulse(up, 0, up), 0);
        assert_eq!(missed_click_pulse(0, 0, 0), 0);
    }

    #[test]
    fn runtime_mode_labels_do_not_claim_a_sim_menu_or_enable_hle() {
        let plain = super::RuntimeMode::default();
        assert!(plain.label().contains("no SIM"));
        assert!(plain.label().contains("DSP boot HLE: off"));
        assert!(plain.notice().contains("DSP boot HLE is off"));
        let no_card = super::RuntimeMode { boot_hle: true, ..plain };
        assert!(no_card.notice().contains("Insert SIM / SOS"));
        assert!(no_card.notice().contains("App-menu navigation is not verified"));
        let candidate = super::RuntimeMode { sim: true, rf_hle: true, battery_hle: true, ..no_card };
        assert!(candidate.label().contains("experimental SIM"));
        assert!(candidate.label().contains("empty air"));
        assert!(candidate.label().contains("3.7 V"));
        assert!(candidate.notice().contains("Experimental SIM boot is incomplete"));
        assert!(candidate.notice().contains("not a verified SIM standby or app menu"));
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
