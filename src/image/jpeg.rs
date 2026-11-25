use super::CapturedImage;
use crate::android::JniBridge;

/// Saves the untouched JPEG bytes produced by the camera.
///
/// No decode, resize, re-encode, sharpening, smoothing, beauty mode, colour
/// filter, or computational/AI enhancement happens here. The bytes are only
/// structurally validated before being copied to MediaStore.
pub fn save_jpeg(jni: &JniBridge, image: &CapturedImage) -> Result<String, String> {
    if image.format != super::CaptureFormat::Jpeg {
        return Err("not a JPEG capture".into());
    }
    validate_jpeg(&image.bytes)?;
    crate::android::mediastore::save_to_pictures(jni, &image.bytes, "image/jpeg", "jpg")
}

/// Minimal structural check: SOI, EOI, bounded segments, SOF and SOS.
fn validate_jpeg(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return Err("camera output is not a JPEG stream".into());
    }
    if bytes[bytes.len() - 2] != 0xff || bytes[bytes.len() - 1] != 0xd9 {
        return Err("JPEG stream has no EOI marker".into());
    }

    let mut cursor = 2usize;
    let mut saw_frame = false;
    let mut saw_scan = false;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] != 0xff {
            return Err("malformed JPEG marker".into());
        }
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Err("truncated JPEG marker".into());
        }

        let marker = bytes[cursor];
        cursor += 1;

        match marker {
            0x01 | 0xd0..=0xd7 => continue, // TEM / restart markers
            0xd9 => break,                  // EOI
            0xda => {                       // SOS
                if !saw_frame {
                    return Err("JPEG scan appears before frame header".into());
                }
                saw_scan = true;
                break;
            }
            _ => {
                if cursor + 2 > bytes.len() {
                    return Err("truncated JPEG segment".into());
                }
                let length = ((bytes[cursor] as usize) << 8) | bytes[cursor + 1] as usize;
                if length < 2 {
                    return Err("invalid JPEG segment length".into());
                }
                let end = cursor
                    .checked_add(length)
                    .ok_or_else(|| "invalid JPEG segment length".to_string())?;
                // Reserve the final EOI marker from the segment range.
                if end > bytes.len() - 2 {
                    return Err("truncated JPEG segment".into());
                }
                if is_frame_marker(marker) {
                    saw_frame = true;
                }
                cursor = end;
            }
        }
    }

    if !saw_frame || !saw_scan {
        return Err("JPEG is missing frame or scan data".into());
    }
    Ok(())
}

fn is_frame_marker(marker: u8) -> bool {
    matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc)
}
