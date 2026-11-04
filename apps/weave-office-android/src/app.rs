//! The Android entry point: GameActivity starts android_main on its own thread once the
//! library is loaded.

use std::path::PathBuf;

use eframe::egui;
use weave_office::app::WeaveApp;
use winit::platform::android::activity::AndroidApp;
use winit::platform::android::activity::input::{ImeOptions, InputType, TextInputAction, TextInputState, TextSpan};

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

    // The Activity answers the host's document pickers through the file bridge.
    let bridge = std::sync::Arc::new(
        crate::bridge::Bridge::new(&files_dir).map_err(|e| format!("couldn't create the file bridge: {e}"))?,
    );
    weave_office::clipboard::android::install(weave_office::clipboard::android::Clipboard {
        set: {
            let bridge = bridge.clone();
            Box::new(move |text| bridge.set_clipboard(text))
        },
        get: {
            let bridge = bridge.clone();
            Box::new(move || bridge.clipboard_text())
        },
        revision: {
            let bridge = bridge.clone();
            Box::new(move || bridge.clipboard_revision())
        },
    });
    weave_office::file_dialog::android::install(weave_office::file_dialog::android::Picker {
        request: {
            let bridge = bridge.clone();
            Box::new(move |mode, options| bridge.request(mode, options))
        },
        poll: {
            let bridge = bridge.clone();
            Box::new(move |id| bridge.poll(id))
        },
    });

    let shell_android = android.clone();
    let shell_bridge = bridge.clone();
    let pad = crate::pad::Pad::new(&files_dir);
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
        Box::new(move |cc| {
            let mut app = WeaveApp::new(cc, Vec::new());
            app.enable_recovery();
            Ok(Box::new(Shell {
                app,
                android: shell_android,
                bridge: shell_bridge,
                ime: String::new(),
                editing: false,
                pad,
                touch: crate::touch::TouchScroll::new(),
                layout_checked: false,
                print_pending: None,
            }))
        }),
    )
    .map_err(|e| e.to_string())
}

/// The shell around the app: the soft keyboard's text, which winit drops on Android.
struct Shell {
    app: WeaveApp,
    android: AndroidApp,
    /// The file bridge: the Activity's answers and the documents other apps sent.
    bridge: std::sync::Arc<crate::bridge::Bridge>,
    /// The keyboard's text as of the last frame.
    ime: String,
    /// Whether a text field had focus last frame.
    editing: bool,
    /// The Shortcut Pad: touch keys and a wheel over the window's edge.
    pad: crate::pad::Pad,
    /// Two-finger scrolling, which winit's Android backend doesn't emit.
    touch: crate::touch::TouchScroll,
    /// Whether the phone-or-tablet layout has been chosen for this window.
    layout_checked: bool,
    /// The PDF the app is exporting for a print job, until the Activity takes it.
    print_pending: Option<PathBuf>,
}

impl Shell {
    /// Set the keyboard up for a text field and empty its buffer.
    fn reset_keyboard(&mut self) {
        self.android.set_ime_editor_info(
            InputType::TYPE_CLASS_TEXT | InputType::TYPE_TEXT_FLAG_MULTI_LINE,
            TextInputAction::None,
            ImeOptions::IME_FLAG_NO_FULLSCREEN,
        );
        self.android.set_text_input_state(TextInputState {
            text: String::new(),
            selection: TextSpan { start: 0, end: 0 },
            compose_region: None,
        });
        self.ime.clear();
    }

    /// The on-screen keyboard's text, as egui events (winit drops GameActivity's text events).
    fn ime_events(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let editing = ctx.egui_wants_keyboard_input();
        if editing && !self.editing {
            self.reset_keyboard();
        }
        if !editing && self.editing {
            // Focus left the field (Enter, a click elsewhere): put the keyboard away, which
            // winit doesn't, so it stops covering the dialog's buttons.
            self.android.hide_soft_input(false);
        }
        self.editing = editing;
        if !editing {
            return;
        }
        let now = self.android.text_input_state().text;
        if now != self.ime {
            let edit = crate::ime::diff(&self.ime, &now);
            raw.events.extend(crate::ime::events(&edit));
            self.ime = now;
        }
        // GameActivity's text events don't wake the UI (winit drops them): poll while typing.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    /// Print the active document: export it to a PDF in the bridge, then let the Activity hand
    /// it to Android's print service.
    fn begin_print(&mut self) {
        let path = self.bridge.print_path();
        self.print_pending = Some(path.clone());
        self.app.export_pdf_to(path);
    }
}

impl eframe::App for Shell {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.app.clear_color(visuals)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.ime_events(ctx, raw);
        self.touch.apply(raw);
        self.app.raw_input_hook(ctx, raw);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx, frame);
        if let Some(action) = self.pad.take_pending() {
            if action == "android.print" {
                self.begin_print();
            } else {
                self.app.run_shell_command(ctx, &action);
            }
        }
        // The PDF a print job is waiting for: hand it over once the export wrote it.
        if let Some(path) = &self.print_pending {
            if std::fs::metadata(path).map(|meta| meta.len() > 0).unwrap_or(false) {
                self.bridge.request_print(path);
                self.print_pending = None;
            }
        }
        // Documents another app sent ("Open with" / "Share").
        for path in self.bridge.take_incoming() {
            self.app.open_external(path);
        }
        // A phone-class screen gets the interface at 75%, Android's own phone/tablet line.
        if !self.layout_checked {
            self.layout_checked = true;
            let screen = ctx.viewport_rect();
            log::info!(
                "layout: ppp={} viewport={:?} content={:?}",
                ctx.pixels_per_point(),
                ctx.viewport_rect(),
                ctx.content_rect()
            );
            if screen.width().min(screen.height()) < 600.0 {
                ctx.set_zoom_factor(0.75);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        // The pad draws over the app's edge, so it is on top of the window.
        self.pad.show(ui.ctx());
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.app.on_exit(gl);
    }
}

/// Where the app keeps its data when Android names no files directory (never, in practice).
fn fallback_files_dir() -> PathBuf {
    std::env::temp_dir().join("weave-office")
}
