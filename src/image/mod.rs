pub mod dng;
pub mod jpeg;

use crate::camera::Capabilities;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFormat {
    Jpeg,
    Raw16,
}

#[derive(Clone, Debug)]
pub struct CapturedImage {
    pub format: CaptureFormat,
    pub bytes: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub row_stride: i32,
    /// Capabilities of the camera that actually produced this image.
    pub caps: Arc<Capabilities>,
    /// Device rotation at capture time, used for DNG orientation metadata.
    pub device_rotation: i32,
}

pub use dng::build_dng;
pub use jpeg::save_jpeg;
