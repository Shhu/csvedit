// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod columns;
mod config;
mod i18n;
mod import;
mod table;
mod theme;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    i18n::init();
    columns::init();
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    let viewport = egui::ViewportBuilder::default()
        .with_title(app::APP_NAME)
        // Window and taskbar icon; on Wayland the icon comes from the
        // .desktop file matching this app id.
        .with_icon(
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                .expect("valid icon PNG"),
        )
        .with_app_id("io.github.Shhu.CsvEdit")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([720.0, 420.0])
        .with_drag_and_drop(true)
        // Own title bar (src/app/titlebar.rs): no system frame.
        .with_decorations(false);
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    // On Windows, Direct3D 12 rather than Vulkan: with Vulkan, resizing the
    // window can leave its surface stuck at the old size (seen on NVIDIA).
    // OpenGL stays as a last resort for GPUs without D3D12.
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends =
            eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::GL;
    }
    eframe::run_native(
        app::APP_NAME,
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::CsvApp::new(path)))
        }),
    )
}
