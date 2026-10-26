#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod runner;

fn main() -> anyhow::Result<()> {
    let root = std::env::current_exe()?.parent().unwrap().to_path_buf();
    let root = if root.join("firmware").is_dir() {
        root
    } else {
        std::env::current_dir()?
    };
    let config = app::Settings::load(&root);
    let auto_run = std::env::args().any(|argument| argument == "--run");
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([640.0, 420.0])
            .with_fullscreen(config.fullscreen)
            .with_title("Samsung E830 Emulator"),
        ..Default::default()
    };
    eframe::run_native(
        "Samsung E830 Emulator",
        native_options,
        Box::new(move |_cc| {
            let mut launcher = app::E830Launcher::new(root, config);
            if auto_run { launcher.run_selected_firmware(); }
            Box::new(launcher)
        }),
    ).map_err(|error| anyhow::anyhow!("eframe: {error}"))
}
