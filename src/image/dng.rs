//! Minimal DNG (TIFF/EP) container writer for RAW_SENSOR captures.
//!
//! This does not synthesize sensor data. It writes the original 16-bit raw
//! bytes together with the white balance, colour and CFA metadata reported by
//! the camera that produced the capture. Devices that do not expose RAW_SENSOR
//! use high-quality native JPEG instead; a JPEG is never presented as raw.

use super::{CaptureFormat, CapturedImage};
use crate::camera::Capabilities;

const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;
const TYPE_SRATIONAL: u16 = 10;

// Reserve space for at most 32 IFD entries. External values start after it.
const IFD_RESERVED_OFFSET: usize = 8 + 2 + 32 * 12 + 4;
const IFD_START: usize = 8;

struct Entry {
    tag: u16,
    type_: u16,
    count: u32,
    value: [u8; 4],
}

struct Values {
    bytes: Vec<u8>,
}

impl Values {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn append(&mut self, data: &[u8], alignment: usize) -> [u8; 4] {
        if data.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..data.len()].copy_from_slice(data);
            return inline;
        }
        self.append_external(data, alignment)
    }

    /// Always stores the value outside the IFD. Required when the value itself
    /// is an offset (StripOffsets) so it can never be mistaken for inline data.
    fn append_external(&mut self, data: &[u8], alignment: usize) -> [u8; 4] {
        while (IFD_RESERVED_OFFSET + self.bytes.len()) % alignment != 0 {
            self.bytes.push(0);
        }
        let offset = (IFD_RESERVED_OFFSET + self.bytes.len()) as u32;
        self.bytes.extend_from_slice(data);
        offset.to_le_bytes()
    }
}

pub fn build_dng(image: &CapturedImage, orientation: i32) -> Result<Vec<u8>, String> {
    if image.format != CaptureFormat::Raw16 {
        return Err("not a RAW_SENSOR capture".into());
    }
    if image.width <= 0 || image.height <= 0 {
        return Err("invalid raw image dimensions".into());
    }

    let caps = image.caps.as_ref();
    let width = image.width as u32;
    let height = image.height as u32;
    let raw = strip_padding(image)?;
    let raw_len = u32::try_from(raw.len()).map_err(|_| "raw image too large for TIFF".to_string())?;

    let mut values = Values::new();
    let mut entries = Vec::new();

    entries.push(short_entry(254, &[0])); // NewSubfileType
    entries.push(long_entry(256, &[width])); // ImageWidth
    entries.push(long_entry(257, &[height])); // ImageLength
    entries.push(short_entry(258, &[16])); // BitsPerSample
    entries.push(short_entry(259, &[1])); // Compression = none
    entries.push(short_entry(262, &[32803])); // PhotometricInterpretation = CFA
    entries.push(ascii_entry(&mut values, 271, "PureCamera")); // Make
    entries.push(ascii_entry(&mut values, 272, &caps.camera_id)); // Model
    entries.push(short_entry(274, &[tiff_orientation(orientation)]));

    entries.push(Entry {
        tag: 273,
        type_: TYPE_LONG,
        count: 1,
        value: values.append_external(&raw, 2),
    }); // StripOffsets
    entries.push(short_entry(277, &[1])); // SamplesPerPixel
    entries.push(long_entry(278, &[height])); // RowsPerStrip
    entries.push(long_entry(279, &[raw_len])); // StripByteCounts
    entries.push(short_entry(284, &[1])); // PlanarConfiguration
    entries.push(short_entry(33421, &[2, 2])); // CFARepeatPatternDim
    entries.push(byte_entry(33422, &cfa_pattern(caps.cfa_arrangement)));

    entries.push(byte_entry(50706, &[1, 4, 0, 0])); // DNGVersion
    entries.push(byte_entry(50707, &[1, 1, 0, 0])); // DNGBackwardVersion
    entries.push(ascii_entry(&mut values, 50708, &format!("PureCamera {}", caps.camera_id)));

    let matrix = colour_matrix(caps);
    let matrix_bytes = srational_bytes(&matrix);
    entries.push(Entry {
        tag: 50721,
        type_: TYPE_SRATIONAL,
        count: 9,
        value: values.append(&matrix_bytes, 4),
    });

    let neutral = as_shot_neutral(caps);
    let neutral_bytes = rational_bytes(&neutral);
    entries.push(Entry {
        tag: 50728,
        type_: TYPE_RATIONAL,
        count: 3,
        value: values.append(&neutral_bytes, 4),
    });

    entries.push(short_entry(50778, &[21])); // CalibrationIlluminant1 = D65

    // BlackLevelRepeatDim and BlackLevel preserve the 2x2 CFA-aligned pattern
    // instead of collapsing it into one averaged scalar.
    let black = rational_bytes(&black_levels(caps));
    entries.push(Entry {
        tag: 50714,
        type_: TYPE_RATIONAL,
        count: 4,
        value: values.append(&black, 4),
    });
    entries.push(long_entry(50717, &[caps.white_level.max(1) as u32])); // WhiteLevel
    entries.push(short_entry(50720, &[width.min(65535) as u16, height.min(65535) as u16]));
    let active_area_value = {
        let mut bytes = Vec::with_capacity(16);
        for value in [0u32, 0, width, height] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        values.append_external(&bytes, 4)
    };
    entries.push(Entry {
        tag: 50829,
        type_: TYPE_LONG,
        count: 4,
        value: active_area_value,
    }); // ActiveArea
    entries.push(short_entry(50713, &[2, 2])); // BlackLevelRepeatDim
    entries.push(ascii_entry(&mut values, 305, "PureCamera (Rust)"));

    if entries.len() > 32 {
        return Err("too many DNG tags".into());
    }
    entries.sort_by_key(|entry| entry.tag);

    let mut tiff = Vec::with_capacity(IFD_RESERVED_OFFSET + values.bytes.len());
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&(IFD_START as u32).to_le_bytes());
    tiff.resize(IFD_RESERVED_OFFSET, 0);

    tiff[IFD_START..IFD_START + 2].copy_from_slice(&(entries.len() as u16).to_le_bytes());
    for (index, entry) in entries.iter().enumerate() {
        let offset = IFD_START + 2 + index * 12;
        tiff[offset..offset + 2].copy_from_slice(&entry.tag.to_le_bytes());
        tiff[offset + 2..offset + 4].copy_from_slice(&entry.type_.to_le_bytes());
        tiff[offset + 4..offset + 8].copy_from_slice(&entry.count.to_le_bytes());
        tiff[offset + 8..offset + 12].copy_from_slice(&entry.value);
    }
    let next_ifd = IFD_START + 2 + entries.len() * 12;
    tiff[next_ifd..next_ifd + 4].copy_from_slice(&0u32.to_le_bytes());

    tiff.extend_from_slice(&values.bytes);
    Ok(tiff)
}

/// Removes row padding from an `AImage` RAW16 plane so the TIFF strip is tight.
///
/// The source plane may end either with a full padded row or with only the
/// pixel bytes of the final row. Anything else is rejected instead of writing
/// padding as if it were sensor pixels.
fn strip_padding(image: &CapturedImage) -> Result<Vec<u8>, String> {
    let tight_row = (image.width as usize)
        .checked_mul(2)
        .ok_or_else(|| "raw row size overflow".to_string())?;
    let height = image.height as usize;
    let stride = usize::try_from(image.row_stride)
        .map_err(|_| "invalid RAW row stride".to_string())?;
    if stride == 0 {
        return Err("RAW row stride is zero".into());
    }
    if stride < tight_row {
        return Err("RAW row stride is smaller than the pixel row".into());
    }

    let full_plane = stride
        .checked_mul(height)
        .ok_or_else(|| "raw plane size overflow".to_string())?;
    let last_row_tight = stride
        .checked_mul(height.saturating_sub(1))
        .and_then(|value| value.checked_add(tight_row))
        .ok_or_else(|| "raw plane size overflow".to_string())?;

    if image.bytes.len() != full_plane && image.bytes.len() != last_row_tight {
        return Err(format!(
            "unexpected RAW plane length: {} (expected {} or {})",
            image.bytes.len(),
            full_plane,
            last_row_tight
        ));
    }

    let mut result = Vec::with_capacity(
        tight_row
            .checked_mul(height)
            .ok_or_else(|| "raw plane size overflow".to_string())?,
    );
    for row in 0..height {
        let start = row
            .checked_mul(stride)
            .ok_or_else(|| "raw row offset overflow".to_string())?;
        let end = start
            .checked_add(tight_row)
            .ok_or_else(|| "raw row offset overflow".to_string())?;
        if end > image.bytes.len() {
            return Err("RAW plane is truncated".into());
        }
        result.extend_from_slice(&image.bytes[start..end]);
    }
    Ok(result)
}

fn tiff_orientation(android_orientation: i32) -> u16 {
    match android_orientation.rem_euclid(360) {
        90 => 6,
        180 => 3,
        270 => 8,
        _ => 1,
    }
}

fn cfa_pattern(arrangement: i32) -> [u8; 4] {
    // Android SENSOR_INFO_COLOR_FILTER_ARRANGEMENT values are RGGB=0,
    // GRBG=1, GBRG=2, BGGR=3. DNG CFAPattern uses 0=red, 1=green, 2=blue
    // in the same top-left 2x2 reading order.
    match arrangement {
        0 => [0, 1, 1, 2], // RGGB
        1 => [1, 0, 2, 1], // GRBG
        2 => [1, 2, 0, 1], // GBRG
        3 => [2, 1, 1, 0], // BGGR
        _ => [0, 1, 1, 2], // Safe Bayer fallback for unknown/non-Bayer sensors.
    }
}

fn black_levels(caps: &Capabilities) -> [(u32, u32); 4] {
    let mut result = [(0u32, 1u32); 4];
    for (index, level) in caps.black_level_pattern.iter().enumerate().take(4) {
        result[index] = ((*level).max(0) as u32, 1);
    }
    result
}

fn colour_matrix(caps: &Capabilities) -> [(i32, i32); 9] {
    let source = if caps.forward_matrix1.len() >= 9 {
        &caps.forward_matrix1[..9]
    } else if caps.calibration1.len() >= 9 {
        &caps.calibration1[..9]
    } else if caps.color_transform.len() >= 9 {
        &caps.color_transform[..9]
    } else {
        return [
            (1, 1), (0, 1), (0, 1),
            (0, 1), (1, 1), (0, 1),
            (0, 1), (0, 1), (1, 1),
        ];
    };
    let mut result = [(0i32, 1i32); 9];
    for (index, value) in source.iter().enumerate() {
        result[index] = float_to_srational(*value, 10_000);
    }
    result
}

fn as_shot_neutral(caps: &Capabilities) -> [(u32, u32); 3] {
    if caps.gains.len() >= 3 {
        let mut result = [(1u32, 1u32); 3];
        for (index, value) in caps.gains.iter().take(3).enumerate() {
            result[index] = positive_rational(*value, 10_000);
        }
        result
    } else {
        [(1, 1), (1, 1), (1, 1)]
    }
}

fn float_to_srational(value: f32, denominator: i32) -> (i32, i32) {
    ((value * denominator as f32).round() as i32, denominator)
}

fn positive_rational(value: f32, denominator: u32) -> (u32, u32) {
    let numerator = (value.max(0.0) * denominator as f32).round().max(0.0) as u32;
    (numerator, denominator)
}

fn short_entry(tag: u16, values: &[u16]) -> Entry {
    let mut bytes = Vec::with_capacity(values.len() * 2);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Entry { tag, type_: TYPE_SHORT, count: values.len() as u32, value: inline(&bytes) }
}

fn long_entry(tag: u16, values: &[u32]) -> Entry {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Entry { tag, type_: TYPE_LONG, count: values.len() as u32, value: inline(&bytes) }
}

fn byte_entry(tag: u16, values: &[u8]) -> Entry {
    Entry { tag, type_: TYPE_BYTE, count: values.len() as u32, value: inline(values) }
}

fn ascii_entry(values: &mut Values, tag: u16, text: &str) -> Entry {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    let value = values.append(&bytes, 1);
    Entry { tag, type_: TYPE_ASCII, count: bytes.len() as u32, value }
}

fn rational_bytes(values: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 8);
    for (numerator, denominator) in values {
        bytes.extend_from_slice(&numerator.to_le_bytes());
        bytes.extend_from_slice(&denominator.to_le_bytes());
    }
    bytes
}

fn srational_bytes(values: &[(i32, i32)]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 8);
    for (numerator, denominator) in values {
        bytes.extend_from_slice(&numerator.to_le_bytes());
        bytes.extend_from_slice(&denominator.to_le_bytes());
    }
    bytes
}

fn inline(bytes: &[u8]) -> [u8; 4] {
    let mut value = [0u8; 4];
    let len = bytes.len().min(4);
    value[..len].copy_from_slice(&bytes[..len]);
    value
}
