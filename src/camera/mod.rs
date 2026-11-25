//! Camera2 (NDK) subsystem. UI and rendering code never touch NDK pointers.

mod characteristics;
mod controller;
mod device;
mod error;
mod manager;
mod requests;
mod session;

pub use characteristics::Capabilities;
pub use controller::CameraController;
pub use requests::{image_orientation, FlashMode};
pub use session::CaptureQueue;
