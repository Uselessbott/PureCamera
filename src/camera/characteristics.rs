//! Read-only camera characteristics, parsed from `ACameraMetadata`.

use super::error::{ CameraError, Result };
use crate::config::{formats, modes, tags};
use ndk_sys::{
    acamera_metadata_tag, ACameraMetadata, ACameraMetadata_const_entry, ACAMERA_TYPE_BYTE,
    ACAMERA_TYPE_DOUBLE, ACAMERA_TYPE_FLOAT, ACAMERA_TYPE_INT32, ACAMERA_TYPE_RATIONAL,
};
use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::slice;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub fn area(self) -> i64 {
        self.width as i64 * self.height as i64
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SensorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl SensorRect {
    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }

    /// Crops this active array rectangle around its centre.
    pub fn cropped(self, zoom: f32) -> SensorRect {
        let zoom = zoom.clamp(1.0, 64.0);
        let width = ((self.width() as f32) / zoom).round().max(1.0) as i32;
        let height = ((self.height() as f32) / zoom).round().max(1.0) as i32;
        let left = self.left + (self.width() - width) / 2;
        let top = self.top + (self.height() - height) / 2;
        SensorRect { left, top, right: left + width, bottom: top + height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LensFacing {
    Front,
    Back,
    External,
    Unknown,
}

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub camera_id: String,
    pub sensor_orientation: i32,
    pub lens_facing: LensFacing,
    pub active_array: SensorRect,
    pub pixel_array: Size,
    pub max_digital_zoom: f32,
    pub preview_sizes: Vec<Size>,
    pub jpeg_sizes: Vec<Size>,
    pub raw_sizes: Vec<Size>,
    pub exposure_range: (i32, i32),
    pub compensation_step: f32,
    pub af_modes: Vec<i32>,
    pub noise_modes: Vec<i32>,
    pub edge_modes: Vec<i32>,
    pub shading_modes: Vec<i32>,
    pub tonemap_modes: Vec<i32>,
    pub flash_available: bool,
    pub raw_capable: bool,
    pub manual_post_processing: bool,
    pub cfa_arrangement: i32,
    pub white_level: i32,
    pub black_level: i32,
    /// 2x2 CFA-aligned black level pattern from SENSOR_BLACK_LEVEL_PATTERN.
    pub black_level_pattern: [i32; 4],
    pub focal_length: f32,
    pub physical_size: [f32; 2],
    pub forward_matrix1: Vec<f32>,
    pub calibration1: Vec<f32>,
    pub color_transform: Vec<f32>,
    pub gains: Vec<f32>,
    pub noise_profile: Vec<f32>,
}

impl Default for LensFacing {
    fn default() -> Self {
        LensFacing::Unknown
    }
}

impl Capabilities {
    pub unsafe fn from_metadata(
        metadata: *const ACameraMetadata,
        camera_id: *const std::os::raw::c_char,
    ) -> Result<Self> {
        if metadata.is_null() {
            return Err(CameraError::NullPointer);
        }

        let id = if camera_id.is_null() {
            String::new()
        } else {
            CStr::from_ptr(camera_id).to_string_lossy().into_owned()
        };

        let active_values = i32_values(metadata, tags::sensor_active_array());
        let active_array = if active_values.len() >= 4 {
            SensorRect {
                left: active_values[0],
                top: active_values[1],
                right: active_values[2],
                bottom: active_values[3],
            }
        } else {
            SensorRect::default()
        };

        let pixel_values = i32_values(metadata, tags::sensor_pixel_array());
        let pixel_array = if pixel_values.len() >= 2 {
            Size { width: pixel_values[0], height: pixel_values[1] }
        } else {
            Size { width: active_array.width(), height: active_array.height() }
        };

        let stream_configs = i32_values(metadata, tags::scaler_available_stream_configurations());
        let mut preview_sizes = Vec::new();
        let mut jpeg_sizes = Vec::new();
        let mut raw_sizes = Vec::new();
        for config in stream_configs.chunks_exact(4) {
            if config[3] != 0 {
                continue; // input stream
            }
            let size = Size { width: config[1], height: config[2] };
            if size.width <= 0 || size.height <= 0 {
                continue;
            }
            match config[0] {
                formats::YUV_420_888 => preview_sizes.push(size),
                formats::JPEG => jpeg_sizes.push(size),
                formats::RAW_SENSOR => raw_sizes.push(size),
                _ => {}
            }
        }
        sort_by_area(&mut preview_sizes);
        sort_by_area(&mut jpeg_sizes);
        sort_by_area(&mut raw_sizes);
        if preview_sizes.is_empty() {
            preview_sizes = jpeg_sizes.clone();
        }

        let zoom = float_values(metadata, tags::scaler_max_digital_zoom())
            .first()
            .copied()
            .unwrap_or(1.0);

        let exposure_values = i32_values(metadata, tags::control_ae_compensation_range());
        let exposure_range = if exposure_values.len() >= 2 {
            (exposure_values[0], exposure_values[1])
        } else {
            (-6, 6)
        };
        let step = rational_value(metadata, tags::control_ae_compensation_step())
            .unwrap_or((1, 1));
        let compensation_step = if step.1 != 0 {
            step.0 as f32 / step.1 as f32
        } else {
            1.0
        };

        let facing = match byte_values(metadata, tags::lens_facing()).first().copied() {
            Some(0) => LensFacing::Front,
            Some(1) => LensFacing::Back,
            Some(2) => LensFacing::External,
            _ => LensFacing::Unknown,
        };

        let capabilities = byte_values(metadata, tags::available_capabilities());
        let raw_capable = !raw_sizes.is_empty()
            || capabilities.contains(&modes::CAPABILITY_RAW);
        let manual_post_processing =
            capabilities.contains(&modes::CAPABILITY_MANUAL_POST_PROCESSING);

        let black_values = i32_values(metadata, tags::sensor_black_level_pattern());
        let black_level = if black_values.is_empty() {
            0
        } else {
            (black_values.iter().sum::<i32>() / black_values.len() as i32).max(0)
        };
        let black_level_pattern = if black_values.len() >= 4 {
            [
                black_values[0].max(0),
                black_values[1].max(0),
                black_values[2].max(0),
                black_values[3].max(0),
            ]
        } else {
            [black_level; 4]
        };

        let physical = float_values(metadata, tags::lens_physical_size());
        let physical_size = if physical.len() >= 2 {
            [physical[0], physical[1]]
        } else {
            [0.0, 0.0]
        };

        Ok(Self {
            camera_id: id,
            sensor_orientation: i32_values(metadata, tags::sensor_orientation())
                .first()
                .copied()
                .unwrap_or(0),
            lens_facing: facing,
            active_array,
            pixel_array,
            max_digital_zoom: if zoom > 1.0 { zoom } else { 1.0 },
            preview_sizes,
            jpeg_sizes,
            raw_sizes,
            exposure_range,
            compensation_step,
            af_modes: byte_values(metadata, tags::control_af_available_modes()),
            noise_modes: byte_values(metadata, tags::noise_reduction_available_modes()),
            edge_modes: byte_values(metadata, tags::edge_available_modes()),
            shading_modes: byte_values(metadata, tags::shading_available_modes()),
            tonemap_modes: byte_values(metadata, tags::tonemap_available_modes()),
            flash_available: byte_values(metadata, tags::flash_info_available())
                .first()
                .copied()
                == Some(1),
            raw_capable,
            manual_post_processing,
            cfa_arrangement: byte_values(metadata, tags::sensor_color_filter_arrangement())
                .first()
                .copied()
                .unwrap_or(0),
            white_level: i32_values(metadata, tags::sensor_white_level())
                .first()
                .copied()
                .unwrap_or(1023),
            black_level,
            black_level_pattern,
            focal_length: float_values(metadata, tags::lens_focal_length())
                .first()
                .copied()
                .unwrap_or(0.0),
            physical_size,
            forward_matrix1: rational_values(metadata, tags::sensor_forward_matrix1()),
            calibration1: rational_values(metadata, tags::sensor_calibration_transform1()),
            color_transform: rational_values(metadata, tags::color_correction_transform()),
            gains: float_values(metadata, tags::color_correction_gains()),
            noise_profile: double_values(metadata, tags::sensor_noise_profile()),
        })
    }

    pub fn largest_jpeg(&self) -> Option<Size> {
        self.jpeg_sizes.first().copied()
    }

    pub fn largest_raw(&self) -> Option<Size> {
        self.raw_sizes.first().copied()
    }

    /// Picks a preview size with an aspect ratio close to the display size.
    pub fn preview_for_display(&self, width: i32, height: i32) -> Option<Size> {
        if width <= 0 || height <= 0 || self.preview_sizes.is_empty() {
            return self.preview_sizes.first().copied();
        }
        let wanted = width as f32 / height as f32;
        self.preview_sizes
            .iter()
            .min_by(|a, b| {
                let da = (a.width as f32 / a.height as f32 - wanted).abs();
                let db = (b.width as f32 / b.height as f32 - wanted).abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
    }
}

fn sort_by_area(sizes: &mut [Size]) {
    sizes.sort_by(|a, b| b.area().cmp(&a.area()));
}

unsafe fn entry(metadata: *const ACameraMetadata, tag: u32) -> Option<ACameraMetadata_const_entry> {
    let mut value = MaybeUninit::<ACameraMetadata_const_entry>::uninit();
    let status = ndk_sys::ACameraMetadata_getConstEntry(metadata, tag, value.as_mut_ptr());
    if status.0 != ndk_sys::camera_status_t::ACAMERA_OK.0 {
        return None;
    }
    Some(value.assume_init())
}

unsafe fn byte_values(metadata: *const ACameraMetadata, tag: u32) -> Vec<i32> {
    match entry(metadata, tag) {
        Some(value) if value.type_ == ACAMERA_TYPE_BYTE as u8 && !value.data.u8_.is_null() => {
            slice::from_raw_parts(value.data.u8_, value.count as usize)
                .iter()
                .map(|value| *value as i32)
                .collect()
        }
        _ => Vec::new(),
    }
}

unsafe fn i32_values(metadata: *const ACameraMetadata, tag: u32) -> Vec<i32> {
    match entry(metadata, tag) {
        Some(value) if value.type_ == ACAMERA_TYPE_INT32 as u8 && !value.data.i32_.is_null() => {
            slice::from_raw_parts(value.data.i32_, value.count as usize).to_vec()
        }
        _ => Vec::new(),
    }
}

unsafe fn float_values(metadata: *const ACameraMetadata, tag: u32) -> Vec<f32> {
    match entry(metadata, tag) {
        Some(value) if value.type_ == ACAMERA_TYPE_FLOAT as u8 && !value.data.f.is_null() => {
            slice::from_raw_parts(value.data.f, value.count as usize).to_vec()
        }
        _ => Vec::new(),
    }
}

unsafe fn double_values(metadata: *const ACameraMetadata, tag: u32) -> Vec<f32> {
    match entry(metadata, tag) {
        Some(value) if value.type_ == ACAMERA_TYPE_DOUBLE as u8 && !value.data.d.is_null() => {
            slice::from_raw_parts(value.data.d, value.count as usize)
                .iter()
                .map(|value| *value as f32)
                .collect()
        }
        _ => Vec::new(),
    }
}

unsafe fn rational_values(metadata: *const ACameraMetadata, tag: u32) -> Vec<f32> {
    match entry(metadata, tag) {
        Some(value)
            if value.type_ == ACAMERA_TYPE_RATIONAL as u8 && !value.data.r.is_null() =>
        {
            slice::from_raw_parts(value.data.r, value.count as usize)
                .iter()
                .map(|r| {
                    if r.denominator != 0 {
                        r.numerator as f32 / r.denominator as f32
                    } else {
                        0.0
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

unsafe fn rational_value(metadata: *const ACameraMetadata, tag: u32) -> Option<(i32, i32)> {
    match entry(metadata, tag) {
        Some(value)
            if value.type_ == ACAMERA_TYPE_RATIONAL as u8 && !value.data.r.is_null() && value.count > 0 =>
        {
            let value = &*value.data.r;
            Some((value.numerator, value.denominator))
        }
        _ => None,
    }
}

// Ensures the bindgen tag newtype is referenced even in builds where `tags`
// is inlined by the optimizer; also documents the expected tag type.
const _: fn(u32) -> acamera_metadata_tag = acamera_metadata_tag;
