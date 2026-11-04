//! The Android entry point: GameActivity starts android_main on its own thread once the
//! library is loaded.

use std::path::PathBuf;

use weave_office::app::WeaveApp;
use winit::platform::android::activity::AndroidApp;

/// android-activity looks the entry point up by name, hence no_mangle.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("WeaveOffice"),
    );
    // Panics end up in logcat rather than vanishing with the process.
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    if let Err(e) = run(app) {
        log::error!("Weave Office stopped: {e}");
    }
}

fn run(android: AndroidApp) -> Result<(), String> {
    let files_dir = android.internal_data_path().unwrap_or_else(fallback_files_dir);
    log::info!("Weave Office {} starting; data in {}", env!("CARGO_PKG_VERSION"), files_dir.display());
    // The host keeps its settings and recovery copies under $HOME/Library/Application Support;
    // point HOME at the app's private storage so nothing tries to write outside it.
    // SAFETY: called before eframe starts any thread, so no other thread reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("HOME", &files_dir);
    }

    let mut options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        // eframe hands it to winit's event loop (with_android_app).
        android_app: Some(android),
        ..Default::default()
    };
    // Vulkan first, GLES on devices without a usable Vulkan driver.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN | eframe::wgpu::Backends::GL;
    }

    eframe::run_native(
        "Weave Office",
        options,
        Box::new(|cc| {
            let mut app = WeaveApp::new(cc, Vec::new());
            app.enable_recovery();
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

/// Where the app keeps its data when Android names no files directory (never, in practice).
fn fallback_files_dir() -> PathBuf {
    std::env::temp_dir().join("weave-office")
}
