use crate::camera::FlashMode;

#[derive(Clone, Debug)]
pub struct UiState {
    pub flash: FlashMode,
    pub raw_mode: bool,
    pub raw_supported: bool,
    pub exposure_steps: i32,
    pub zoom: f32,
    pub front_camera: bool,
    pub capture_requested: bool,
    pub focus_request: Option<(f32, f32)>,
    pub message: Option<String>,
    pub message_at_ms: u64,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            flash: FlashMode::Auto,
            raw_mode: false,
            raw_supported: false,
            exposure_steps: 0,
            zoom: 1.0,
            front_camera: true,
            capture_requested: false,
            focus_request: None,
            message: None,
            message_at_ms: 0,
        }
    }

    pub fn cycle_flash(&mut self) {
        self.flash = match self.flash {
            FlashMode::Auto => FlashMode::On,
            FlashMode::On => FlashMode::Off,
            FlashMode::Off => FlashMode::Auto,
        };
    }

    pub fn set_message(&mut self, message: impl Into<String>, now_ms: u64) {
        self.message = Some(message.into());
        self.message_at_ms = now_ms;
    }
}
