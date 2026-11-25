//! Small, dependency-free constants shared by the camera, renderer and UI.

use ndk_sys::acamera_metadata_tag as tag;

/// Camera2/NDK metadata tags used by this app.
///
/// These are associated constants on `acamera_metadata_tag`. Keeping them in
/// one place avoids magic numbers scattered through the camera code and makes
/// it easy to adapt if an NDK release renames a tag.
pub mod tags {
    use super::tag;

    pub fn sensor_orientation() -> u32 {
        tag::ACAMERA_SENSOR_ORIENTATION.0
    }
    pub fn lens_facing() -> u32 {
        tag::ACAMERA_LENS_FACING.0
    }
    pub fn scaler_available_stream_configurations() -> u32 {
        tag::ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS.0
    }
    pub fn scaler_crop_region() -> u32 {
        tag::ACAMERA_SCALER_CROP_REGION.0
    }
    pub fn scaler_max_digital_zoom() -> u32 {
        tag::ACAMERA_SCALER_AVAILABLE_MAX_DIGITAL_ZOOM.0
    }
    pub fn jpeg_orientation() -> u32 {
        tag::ACAMERA_JPEG_ORIENTATION.0
    }
    pub fn jpeg_quality() -> u32 {
        tag::ACAMERA_JPEG_QUALITY.0
    }
    pub fn jpeg_thumbnail_size() -> u32 {
        tag::ACAMERA_JPEG_THUMBNAIL_SIZE.0
    }
    pub fn control_mode() -> u32 {
        tag::ACAMERA_CONTROL_MODE.0
    }
    pub fn control_ae_mode() -> u32 {
        tag::ACAMERA_CONTROL_AE_MODE.0
    }
    pub fn control_ae_compensation() -> u32 {
        tag::ACAMERA_CONTROL_AE_EXPOSURE_COMPENSATION.0
    }
    pub fn control_ae_compensation_range() -> u32 {
        tag::ACAMERA_CONTROL_AE_COMPENSATION_RANGE.0
    }
    pub fn control_ae_compensation_step() -> u32 {
        tag::ACAMERA_CONTROL_AE_COMPENSATION_STEP.0
    }
    pub fn control_af_mode() -> u32 {
        tag::ACAMERA_CONTROL_AF_MODE.0
    }
    pub fn control_af_available_modes() -> u32 {
        tag::ACAMERA_CONTROL_AF_AVAILABLE_MODES.0
    }
    pub fn control_af_trigger() -> u32 {
        tag::ACAMERA_CONTROL_AF_TRIGGER.0
    }
    pub fn control_af_regions() -> u32 {
        tag::ACAMERA_CONTROL_AF_REGIONS.0
    }
    pub fn control_awb_mode() -> u32 {
        tag::ACAMERA_CONTROL_AWB_MODE.0
    }
    pub fn capture_intent() -> u32 {
        tag::ACAMERA_CONTROL_CAPTURE_INTENT.0
    }
    pub fn flash_mode() -> u32 {
        tag::ACAMERA_FLASH_MODE.0
    }
    pub fn flash_info_available() -> u32 {
        tag::ACAMERA_FLASH_INFO_AVAILABLE.0
    }
    pub fn noise_reduction_mode() -> u32 {
        tag::ACAMERA_NOISE_REDUCTION_MODE.0
    }
    pub fn noise_reduction_available_modes() -> u32 {
        tag::ACAMERA_NOISE_REDUCTION_AVAILABLE_NOISE_REDUCTION_MODES.0
    }
    pub fn edge_mode() -> u32 {
        tag::ACAMERA_EDGE_MODE.0
    }
    pub fn edge_available_modes() -> u32 {
        tag::ACAMERA_EDGE_AVAILABLE_EDGE_MODES.0
    }
    pub fn shading_mode() -> u32 {
        tag::ACAMERA_SHADING_MODE.0
    }
    pub fn shading_available_modes() -> u32 {
        tag::ACAMERA_SHADING_AVAILABLE_MODES.0
    }
    pub fn color_correction_mode() -> u32 {
        tag::ACAMERA_COLOR_CORRECTION_MODE.0
    }
    pub fn color_correction_transform() -> u32 {
        tag::ACAMERA_COLOR_CORRECTION_TRANSFORM.0
    }
    pub fn color_correction_gains() -> u32 {
        tag::ACAMERA_COLOR_CORRECTION_GAINS.0
    }
    pub fn tonemap_mode() -> u32 {
        tag::ACAMERA_TONEMAP_MODE.0
    }
    pub fn tonemap_available_modes() -> u32 {
        tag::ACAMERA_TONEMAP_AVAILABLE_TONE_MAP_MODES.0
    }
    pub fn tonemap_curve_red() -> u32 {
        tag::ACAMERA_TONEMAP_CURVE_RED.0
    }
    pub fn tonemap_curve_green() -> u32 {
        tag::ACAMERA_TONEMAP_CURVE_GREEN.0
    }
    pub fn tonemap_curve_blue() -> u32 {
        tag::ACAMERA_TONEMAP_CURVE_BLUE.0
    }
    pub fn sensor_active_array() -> u32 {
        tag::ACAMERA_SENSOR_INFO_ACTIVE_ARRAY_SIZE.0
    }
    pub fn sensor_pixel_array() -> u32 {
        tag::ACAMERA_SENSOR_INFO_PIXEL_ARRAY_SIZE.0
    }
    pub fn sensor_white_level() -> u32 {
        tag::ACAMERA_SENSOR_INFO_WHITE_LEVEL.0
    }
    pub fn sensor_black_level_pattern() -> u32 {
        tag::ACAMERA_SENSOR_BLACK_LEVEL_PATTERN.0
    }
    pub fn sensor_color_filter_arrangement() -> u32 {
        tag::ACAMERA_SENSOR_INFO_COLOR_FILTER_ARRANGEMENT.0
    }
    pub fn sensor_noise_profile() -> u32 {
        tag::ACAMERA_SENSOR_NOISE_PROFILE.0
    }
    pub fn sensor_forward_matrix1() -> u32 {
        tag::ACAMERA_SENSOR_FORWARD_MATRIX1.0
    }
    pub fn sensor_calibration_transform1() -> u32 {
        tag::ACAMERA_SENSOR_CALIBRATION_TRANSFORM1.0
    }
    pub fn lens_focal_length() -> u32 {
        tag::ACAMERA_LENS_FOCAL_LENGTH.0
    }
    pub fn lens_physical_size() -> u32 {
        tag::ACAMERA_SENSOR_INFO_PHYSICAL_SIZE.0
    }
    pub fn available_capabilities() -> u32 {
        tag::ACAMERA_REQUEST_AVAILABLE_CAPABILITIES.0
    }
}

/// HAL pixel format values used in `SCALER_AVAILABLE_STREAM_CONFIGURATIONS`.
pub mod formats {
    pub const RAW_SENSOR: i32 = 0x20; // HAL_PIXEL_FORMAT_RAW_SENSOR / RAW16
    pub const YUV_420_888: i32 = 0x23;
    pub const JPEG: i32 = 0x100;
}

/// Camera2 enum values where the NDK only exposes a typed wrapper enum.
pub mod modes {
    pub const CONTROL_MODE_AUTO: i32 = 1;
    pub const AE_MODE_ON: i32 = 1;
    pub const AE_MODE_ON_AUTO_FLASH: i32 = 2;
    pub const AE_MODE_ON_ALWAYS_FLASH: i32 = 3;
    pub const FLASH_MODE_OFF: i32 = 0;
    pub const FLASH_MODE_SINGLE: i32 = 1;
    pub const AF_MODE_OFF: i32 = 0;
    pub const AF_MODE_AUTO: i32 = 1;
    pub const AF_MODE_CONTINUOUS_PICTURE: i32 = 4;
    pub const AF_TRIGGER_IDLE: i32 = 0;
    pub const AF_TRIGGER_START: i32 = 1;
    pub const NOISE_REDUCTION_OFF: i32 = 0;
    pub const NOISE_REDUCTION_MINIMAL: i32 = 3;
    pub const EDGE_MODE_OFF: i32 = 0;
    pub const SHADING_MODE_OFF: i32 = 0;
    pub const COLOR_CORRECTION_TRANSFORM_MATRIX: i32 = 0;
    pub const TONEMAP_MODE_CONTRAST_CURVE: i32 = 0;
    pub const CAPABILITY_RAW: i32 = 3;
    pub const CAPABILITY_MANUAL_POST_PROCESSING: i32 = 2;
    pub const CAPTURE_INTENT_STILL_IMAGE: i32 = 1;
    pub const AWB_MODE_AUTO: i32 = 1;
}

pub const PERMISSION_REQUEST_CODE: i32 = 1001;
pub const MAX_ZOOM: f32 = 4.0;
pub const AF_TRIGGER_CLEAR_MS: u64 = 450;
pub const CAMERA_RECOVERY_RETRY_MS: u64 = 750;
pub const JPEG_MAX_IMAGES: i32 = 3;
pub const RAW_MAX_IMAGES: i32 = 2;
pub const MEDIA_SUBDIRECTORY: &str = "Pictures/PureCamera";
