//! Nexus — a music player for your own lossless library.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod acts;
mod analyzer;
mod background;
mod app;
mod covers;
mod icons;
mod importing;
mod instance;
mod integrations;
mod layout;
mod media;
mod netimg;
mod nowpanel;
mod pages;
mod player;
mod table;
mod theme;
mod widgets;

use nexus_core::config::Config;
use nexus_core::paths;
use std::io::Write;

const ICON_PNG: &[u8] = include_bytes!("../../../../icons/nexus-256.png");

/// Appends to crash.log in the data folder (a windowed app has no console to print to).
pub fn log_problem(text: &str) {
    let p = paths::data_dir().join("crash.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[{now}] nexus {}: {text}", nexus_core::VERSION);
    }
}

fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("crash: {info}\n{}", std::backtrace::Backtrace::force_capture());
        eprintln!("{msg}");
        log_problem(&msg);
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("nexus {}", nexus_core::VERSION);
        return;
    }
    let restarted = args.iter().any(|a| a == "--restarted");
    let files: Vec<std::path::PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(std::path::PathBuf::from).filter(|p| p.exists()).map(|p| p.canonicalize().unwrap_or(p)).collect();
    if std::env::var_os("NEXUS_MULTI").is_none() && !instance::claim(restarted) {
        return; // the running one came to the front
    }

    let cfg = Config::load(paths::data_dir().join("config.json"));
    let w = cfg.map("window");
    let get = |k: &str| w.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
    let mini = cfg.s("layout") == "mini";
    let size = if mini { egui::vec2(480.0, 780.0) } else { egui::vec2(get("w").unwrap_or(1280.0).max(600.0), get("h").unwrap_or(820.0).max(480.0)) };
    let mut vp = egui::ViewportBuilder::default()
        .with_title("Nexus")
        .with_app_id("nexus")
        .with_inner_size(size)
        .with_min_inner_size([420.0, 520.0])
        .with_transparent(cfg.f("background_alpha") < 100.0);
    if let (Some(x), Some(y)) = (get("x"), get("y")) {
        vp = vp.with_position([x, y]);
    }
    if w.get("maximized").and_then(|v| v.as_bool()).unwrap_or(false) && !mini {
        vp = vp.with_maximized(true);
    }
    if let Some(i) = icon() {
        vp = vp.with_icon(std::sync::Arc::new(i));
    }
    let opts = eframe::NativeOptions { viewport: vp, renderer: eframe::Renderer::Glow, persist_window: false, ..Default::default() };
    let res = eframe::run_native(
        "Nexus",
        opts,
        Box::new(|cc| {
            egui_extras_install(&cc.egui_ctx);
            instance::set_context(&cc.egui_ctx);
            // logout / kill: close the window properly so the session, stats and settings get saved
            let ctx = cc.egui_ctx.clone();
            let _ = ctrlc::set_handler(move || {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx.request_repaint();
            });
            let mut app = app::Nexus::new(cc);
            if !files.is_empty() {
                app.open_files(files, true);
            }
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = res {
        let msg = format!("couldn't open the window: {e}");
        eprintln!("{msg}");
        log_problem(&msg);
        std::process::exit(1);
    }
}

fn egui_extras_install(ctx: &egui::Context) {
    ctx.options_mut(|o| {
        o.zoom_with_keyboard = true;
    });
}
