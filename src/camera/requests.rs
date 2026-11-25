//! Builds capture requests with deliberately neutral image processing.
//!
//! Camera2 enum tags are metadata type BYTE, not INT32. Writing them with the
//! wrong `ACaptureRequest_setEntry_*` function would be rejected by the NDK, so
//! each helper below matches the tag type reported by the camera metadata.

use super::characteristics::{Capabilities, LensFacing, SensorRect};
use super::error::{status, Result};
use crate::config::{modes, tags};
use ndk_sys::{
    ACameraMetadata_rational, ACaptureRequest, ACaptureRequest_setEntry_float,
    ACaptureRequest_setEntry_i32, ACaptureRequest_setEntry_rational,
    ACaptureRequest_setEntry_u8,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashMode {
    Auto,
    On,
    Off,
}

#[derive(Clone, Copy, Debug)]
pub struct FocusPoint {
    pub region: [i32; 5],
    pub active: bool,
}

pub fn set_u8(request: *mut ACaptureRequest, tag: u32, values: &[i32]) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    let bytes: Vec<u8> = values.iter().map(|value| *value as u8).collect();
    status(unsafe {
        ACaptureRequest_setEntry_u8(request, tag, bytes.len() as u32, bytes.as_ptr())
    })
}

pub fn set_i32(request: *mut ACaptureRequest, tag: u32, values: &[i32]) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    status(unsafe {
        ACaptureRequest_setEntry_i32(request, tag, values.len() as u32, values.as_ptr())
    })
}

pub fn set_float(request: *mut ACaptureRequest, tag: u32, values: &[f32]) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    status(unsafe {
        ACaptureRequest_setEntry_float(request, tag, values.len() as u32, values.as_ptr())
    })
}

pub fn set_rational(
    request: *mut ACaptureRequest,
    tag: u32,
    values: &[ACameraMetadata_rational],
) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    status(unsafe {
        ACaptureRequest_setEntry_rational(
            request,
            tag,
            values.len() as u32,
            values.as_ptr(),
        )
    })
}

/// Applies settings shared by preview and still requests.
pub fn apply_neutral(
    request: *mut ACaptureRequest,
    caps: &Capabilities,
    flash: FlashMode,
    exposure_steps: i32,
    zoom: f32,
    focus: FocusPoint,
) -> Result<()> {
    set_u8(request, tags::control_mode(), &[modes::CONTROL_MODE_AUTO])?;
    set_u8(request, tags::control_awb_mode(), &[modes::AWB_MODE_AUTO])?;

    let af_mode = if caps.af_modes.contains(&modes::AF_MODE_CONTINUOUS_PICTURE) {
        modes::AF_MODE_CONTINUOUS_PICTURE
    } else if caps.af_modes.contains(&modes::AF_MODE_AUTO) {
        modes::AF_MODE_AUTO
    } else {
        modes::AF_MODE_OFF
    };
    set_u8(request, tags::control_af_mode(), &[af_mode])?;

    // Preview never forces the flash. Auto/On use AE auto-flash only for stills;
    // Off disables AE flash entirely. Devices without a flash simply ignore it.
    let preview_ae = if caps.flash_available && flash != FlashMode::Off {
        modes::AE_MODE_ON_AUTO_FLASH
    } else {
        modes::AE_MODE_ON
    };
    set_u8(request, tags::control_ae_mode(), &[preview_ae])?;
    if caps.flash_available {
        set_u8(request, tags::flash_mode(), &[modes::FLASH_MODE_OFF])?;
    }

    let exposure = exposure_steps.clamp(caps.exposure_range.0, caps.exposure_range.1);
    set_i32(request, tags::control_ae_compensation(), &[exposure])?;

    let crop = caps.active_array.cropped(zoom);
    set_i32(
        request,
        tags::scaler_crop_region(),
        &[crop.left, crop.top, crop.right, crop.bottom],
    )?;

    if focus.active {
        set_i32(request, tags::control_af_regions(), &focus.region)?;
        set_u8(request, tags::control_af_trigger(), &[modes::AF_TRIGGER_START])?;
    } else {
        set_u8(request, tags::control_af_trigger(), &[modes::AF_TRIGGER_IDLE])?;
    }

    // Best-effort neutral processing: only use modes the device reports.
    if caps.noise_modes.contains(&modes::NOISE_REDUCTION_OFF) {
        let _ = set_u8(request, tags::noise_reduction_mode(), &[modes::NOISE_REDUCTION_OFF]);
    } else if caps.noise_modes.contains(&modes::NOISE_REDUCTION_MINIMAL) {
        let _ = set_u8(request, tags::noise_reduction_mode(), &[modes::NOISE_REDUCTION_MINIMAL]);
    }
    if caps.edge_modes.contains(&modes::EDGE_MODE_OFF) {
        let _ = set_u8(request, tags::edge_mode(), &[modes::EDGE_MODE_OFF]);
    }
    if caps.shading_modes.contains(&modes::SHADING_MODE_OFF) {
        let _ = set_u8(request, tags::shading_mode(), &[modes::SHADING_MODE_OFF]);
    }
    if caps.tonemap_modes.contains(&modes::TONEMAP_MODE_CONTRAST_CURVE) {
        let _ = set_u8(request, tags::tonemap_mode(), &[modes::TONEMAP_MODE_CONTRAST_CURVE]);
        let curve = [0.0_f32, 0.0, 1.0, 1.0];
        let _ = set_float(request, tags::tonemap_curve_red(), &curve);
        let _ = set_float(request, tags::tonemap_curve_green(), &curve);
        let _ = set_float(request, tags::tonemap_curve_blue(), &curve);
    }

    // Identity colour correction: no beauty, vibrance or stylistic filter.
    // Some devices reject manual matrix tags under AUTO control; ignore that
    // rather than failing the whole preview/capture request.
    let _ = set_u8(
        request,
        tags::color_correction_mode(),
        &[modes::COLOR_CORRECTION_TRANSFORM_MATRIX],
    );
    let identity = [
        rational(1, 1), rational(0, 1), rational(0, 1),
        rational(0, 1), rational(1, 1), rational(0, 1),
        rational(0, 1), rational(0, 1), rational(1, 1),
    ];
    let _ = set_rational(request, tags::color_correction_transform(), &identity);
    let _ = set_float(request, tags::color_correction_gains(), &[1.0, 1.0, 1.0, 1.0]);

    Ok(())
}

/// Extra settings for a still-image request.
pub fn apply_still(
    request: *mut ACaptureRequest,
    caps: &Capabilities,
    flash: FlashMode,
    device_rotation: i32,
) -> Result<()> {
    if caps.flash_available {
        let ae_mode = match flash {
            FlashMode::Auto => modes::AE_MODE_ON_AUTO_FLASH,
            FlashMode::On => modes::AE_MODE_ON_ALWAYS_FLASH,
            FlashMode::Off => modes::AE_MODE_ON,
        };
        let flash_mode = if flash == FlashMode::On {
            modes::FLASH_MODE_SINGLE
        } else {
            modes::FLASH_MODE_OFF
        };
        set_u8(request, tags::control_ae_mode(), &[ae_mode])?;
        set_u8(request, tags::flash_mode(), &[flash_mode])?;
    } else {
        set_u8(request, tags::control_ae_mode(), &[modes::AE_MODE_ON])?;
    }
    set_u8(
        request,
        tags::capture_intent(),
        &[modes::CAPTURE_INTENT_STILL_IMAGE],
    )?;
    set_i32(
        request,
        tags::jpeg_orientation(),
        &[image_orientation(caps, device_rotation)],
    )?;
    set_u8(request, tags::jpeg_quality(), &[100])?;
    set_i32(request, tags::jpeg_thumbnail_size(), &[0, 0])?;
    Ok(())
}

/// Degrees needed to rotate the captured image upright for this camera.
pub fn image_orientation(caps: &Capabilities, device_rotation: i32) -> i32 {
    let rotation = device_rotation.rem_euclid(360);
    if caps.lens_facing == LensFacing::Front {
        (caps.sensor_orientation + rotation).rem_euclid(360)
    } else {
        (caps.sensor_orientation - rotation).rem_euclid(360)
    }
}

/// Maps a normalized preview coordinate to the sensor active array.
pub fn focus_region(
    normalized_x: f32,
    normalized_y: f32,
    crop: SensorRect,
) -> [i32; 5] {
    let x = normalized_x.clamp(0.0, 1.0);
    let y = normalized_y.clamp(0.0, 1.0);
    let width = crop.width().max(1);
    let height = crop.height().max(1);
    let centre_x = crop.left + (x * width as f32).round() as i32;
    let centre_y = crop.top + (y * height as f32).round() as i32;

    // A region covering roughly 10% of the sensor, clamped to the crop.
    let half_w = (width / 20).max(1);
    let half_h = (height / 20).max(1);
    let left = centre_x.saturating_sub(half_w).max(crop.left);
    let top = centre_y.saturating_sub(half_h).max(crop.top);
    let right = (centre_x + half_w).min(crop.right);
    let bottom = (centre_y + half_h).min(crop.bottom);
    [left, top, right, bottom, 1000]
}

fn rational(numerator: i32, denominator: i32) -> ACameraMetadata_rational {
    ACameraMetadata_rational { numerator, denominator }
}
