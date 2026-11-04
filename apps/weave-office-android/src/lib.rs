//! Weave Office on Android tablets.
//!
//! Runs the same [`weave_office::app::WeaveApp`] as the desktop app through eframe's native
//! runner on winit's GameActivity backend (wgpu: Vulkan, or GLES where Vulkan is missing). The
//! Rust library is `libweave_office_android.so`; the Gradle project in `android/` packages it
//! with a small Activity (`WeaveOfficeActivity.java`) that adds what winit doesn't offer:
//! document pickers, documents sent from other apps, the pen's tool type and the soft keyboard.
//!
//! The shell follows the ArtCraft Android set's five-layer adaptation stack: a generated
//! launcher, JNI bridges (SAF, IME, clipboard), vendored dependency patches, build-time
//! patches and shared templates.

#[cfg(target_os = "android")]
mod app;

pub mod bridge;

#[cfg(target_os = "android")]
pub mod ime;

#[cfg(target_os = "android")]
pub use app::android_main;
